package router

import (
	"fmt"
	"net"
	"net/http"
	"net/http/httputil"
	"net/url"
	"os"
	"strconv"
	"strings"

	"github.com/QuantumNous/new-api/common"

	"github.com/gin-gonic/gin"
)

// Environment variables wiring the in-repo services under services/ to the
// gateway. Each one names the service's own listener.
const (
	envInvitationsServiceURL   = "INVITATIONS_SERVICE_URL"
	envConnectServiceURL       = "CONNECT_SERVICE_URL"
	envAIBridgeServiceURL      = "AI_BRIDGE_SERVICE_URL"
	envAIBridgeRelayEnabled    = "AI_BRIDGE_RELAY_ENABLED"
	envAIBridgeUpstreamBaseURL = "AI_BRIDGE_UPSTREAM_BASE_URL"
)

// defaultAIBridgeUpstream is ai-bridge's own default for the service it fronts.
const defaultAIBridgeUpstream = "http://127.0.0.1:3000"

// A bundled service owns only its API and protocol paths. Everything else under
// the same prefix is a dashboard page of the embedded frontend: /connect and
// /ai-bridge are rendered natively, so the services' own HTML consoles are not
// reachable through the gateway.
var (
	invitationsServicePaths = []string{"/invitations/api"}
	connectServicePaths     = []string{
		"/connect/api",
		"/connect/authorize",
		"/connect/token",
		"/connect/userinfo",
		"/connect/.well-known",
		"/connect/auth",
		"/connect/clients",
	}
	aiBridgeServicePaths = []string{"/ai-bridge/api", "/ai-bridge/auth"}
)

// relayPathPrefixes are the model protocol entrypoints ai-bridge fronts when
// AI_BRIDGE_RELAY_ENABLED is on, mirroring services/ai-bridge/deploy/nginx-ai-bridge.conf.
var relayPathPrefixes = []string{"/v1", "/v1beta", "/mj", "/suno", "/kling/v1", "/jimeng", "/pg"}

// serviceRoute publishes one group of paths to a bundled service. A route whose
// service is not configured answers with a JSON 404 instead, so a dashboard page
// can tell "not deployed here" apart from a failing service.
type serviceRoute struct {
	service string
	matches func(path string) bool
	// stripPrefix is removed from the path before forwarding; empty keeps it.
	stripPrefix string
	proxy       *httputil.ReverseProxy
}

func registerBundledServices(router *gin.Engine) {
	var routes []serviceRoute
	add := func(envKey, service, stripPrefix string, matches func(string) bool) *url.URL {
		target, err := parseServiceURL(os.Getenv(envKey))
		if err != nil {
			common.SysError(common.LogText("%s is unusable: %v", envKey, err))
			target = nil
		}
		route := serviceRoute{service: service, matches: matches, stripPrefix: stripPrefix}
		if target != nil {
			route.proxy = newServiceProxy(service, target)
			common.SysLog(common.LogText("forward %s requests to %s", service, target))
		}
		routes = append(routes, route)
		return target
	}

	add(envInvitationsServiceURL, "invitations", "/invitations", prefixMatcher(invitationsServicePaths))
	add(envConnectServiceURL, "connect", "", prefixMatcher(connectServicePaths))
	aiBridgeTarget := add(envAIBridgeServiceURL, "ai-bridge", "", prefixMatcher(aiBridgeServicePaths))

	if strings.EqualFold(strings.TrimSpace(os.Getenv(envAIBridgeRelayEnabled)), "true") {
		upstream, loops := relayLoopsBackIntoGateway()
		switch {
		case aiBridgeTarget == nil:
			common.SysError(common.LogText("AI_BRIDGE_RELAY_ENABLED is ignored: %s is empty, so the model protocol entrypoints stay on this gateway", envAIBridgeServiceURL))
		case loops:
			common.SysError(common.LogText("AI_BRIDGE_RELAY_ENABLED is ignored: ai-bridge forwards to %s, which is this gateway, so relay requests would loop", upstream))
		default:
			routes = append(routes, serviceRoute{
				service: "ai-bridge relay",
				matches: relayPathMatcher,
				proxy:   newServiceProxy("ai-bridge relay", aiBridgeTarget),
			})
		}
	}

	router.Use(func(c *gin.Context) {
		path := c.Request.URL.Path
		for _, route := range routes {
			if !route.matches(path) {
				continue
			}
			if route.proxy == nil {
				c.AbortWithStatusJSON(http.StatusNotFound, gin.H{
					"success": false,
					"message": route.service + " service is not configured",
				})
				return
			}
			if route.stripPrefix != "" {
				c.Request.URL.Path = strings.TrimPrefix(path, route.stripPrefix)
				c.Request.URL.RawPath = ""
			}
			route.proxy.ServeHTTP(c.Writer, c.Request)
			c.Abort()
			return
		}
	})
}

// parseServiceURL reads a service listener address; nil means the service is not
// configured on this deployment.
func parseServiceURL(raw string) (*url.URL, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return nil, nil
	}
	target, err := url.Parse(raw)
	if err != nil {
		return nil, err
	}
	if target.Host == "" || (target.Scheme != "http" && target.Scheme != "https") {
		return nil, fmt.Errorf("expected an absolute http:// or https:// address, got %q", raw)
	}
	return target, nil
}

func newServiceProxy(service string, target *url.URL) *httputil.ReverseProxy {
	proxy := httputil.NewSingleHostReverseProxy(target)
	// Stream SSE, image, and video responses without buffering them.
	proxy.FlushInterval = -1
	proxy.ErrorHandler = func(writer http.ResponseWriter, request *http.Request, err error) {
		common.SysError(common.LogText("%s service at %s is unreachable: %v", service, target, err))
		writer.Header().Set("Content-Type", "text/plain; charset=utf-8")
		writer.WriteHeader(http.StatusBadGateway)
		_, _ = writer.Write([]byte(service + " service is unavailable"))
	}
	return proxy
}

func prefixMatcher(prefixes []string) func(string) bool {
	return func(path string) bool {
		for _, prefix := range prefixes {
			if matchesPrefix(path, prefix) {
				return true
			}
		}
		return false
	}
}

// relayPathMatcher also accepts the mode variants new-api exposes, for example
// /turbo/mj/submit/imagine.
func relayPathMatcher(path string) bool {
	if prefixMatcher(relayPathPrefixes)(path) {
		return true
	}
	segments := strings.Split(strings.TrimPrefix(path, "/"), "/")
	return len(segments) >= 2 && segments[0] != "api" && segments[1] == "mj"
}

// relayLoopsBackIntoGateway reports whether ai-bridge forwards back into this
// gateway. Relay routing would then resend every request to itself until the
// client runs out of ports, so the gateway refuses that configuration.
func relayLoopsBackIntoGateway() (string, bool) {
	raw := strings.TrimSpace(os.Getenv(envAIBridgeUpstreamBaseURL))
	if raw == "" {
		raw = defaultAIBridgeUpstream
	}
	upstream, err := url.Parse(raw)
	if err != nil || upstream.Host == "" || upstream.Port() != gatewayPort() {
		return raw, false
	}
	host := strings.TrimSuffix(strings.ToLower(upstream.Hostname()), ".")
	if host == "localhost" {
		return raw, true
	}
	ip := net.ParseIP(host)
	return raw, ip != nil && ip.IsLoopback()
}

// gatewayPort is the port this gateway serves on, resolved the same way main.go
// resolves it.
func gatewayPort() string {
	if port := strings.TrimSpace(os.Getenv("PORT")); port != "" {
		return port
	}
	return strconv.Itoa(*common.Port)
}

// matchesPrefix reports whether path is the prefix itself or a path below it.
func matchesPrefix(path, prefix string) bool {
	return path == prefix || strings.HasPrefix(path, prefix+"/")
}

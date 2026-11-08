package router

import (
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
// gateway. Each one names the service's own listener; when it is empty the
// matching public paths stay on the gateway (embedded dashboard or 404).
const (
	envInvitationsServiceURL   = "INVITATIONS_SERVICE_URL"
	envConnectServiceURL       = "CONNECT_SERVICE_URL"
	envAIBridgeServiceURL      = "AI_BRIDGE_SERVICE_URL"
	envAIBridgeRelayEnabled    = "AI_BRIDGE_RELAY_ENABLED"
	envAIBridgeUpstreamBaseURL = "AI_BRIDGE_UPSTREAM_BASE_URL"
)

// defaultAIBridgeUpstream is ai-bridge's own default for the service it fronts.
const defaultAIBridgeUpstream = "http://127.0.0.1:3000"

// relayPathPrefixes are the model protocol entrypoints ai-bridge fronts when
// AI_BRIDGE_RELAY_ENABLED is on, mirroring services/ai-bridge/deploy/nginx-ai-bridge.conf.
var relayPathPrefixes = []string{"/v1", "/v1beta", "/mj", "/suno", "/kling/v1", "/jimeng", "/pg"}

// rewriteFunc returns the path to send to a service, or false when the public
// path belongs to the gateway instead.
type rewriteFunc func(path string) (string, bool)

type serviceProxyRule struct {
	service string
	target  *url.URL
	rewrite rewriteFunc
	proxy   *httputil.ReverseProxy
}

func registerBundledServices(router *gin.Engine) {
	var rules []serviceProxyRule
	add := func(envKey, service string, rewrite rewriteFunc) {
		raw := strings.TrimSpace(os.Getenv(envKey))
		if raw == "" {
			return
		}
		target, err := url.Parse(raw)
		if err != nil || target.Host == "" || (target.Scheme != "http" && target.Scheme != "https") {
			common.SysError(common.LogText("%s is not a valid service URL: %s", envKey, raw))
			return
		}
		rule := serviceProxyRule{service: service, target: target, rewrite: rewrite}
		rule.proxy = newServiceProxy(service, target)
		rules = append(rules, rule)
		common.SysLog(common.LogText("forward %s requests to %s", service, target))
	}
	add(envInvitationsServiceURL, "invitations", rewriteInvitationsPath)
	add(envConnectServiceURL, "connect", keepPath("/connect"))
	add(envAIBridgeServiceURL, "ai-bridge", keepPath("/ai-bridge"))
	if strings.EqualFold(strings.TrimSpace(os.Getenv(envAIBridgeRelayEnabled)), "true") {
		if upstream, loops := relayLoopsBackIntoGateway(); loops {
			common.SysError(common.LogText("AI_BRIDGE_RELAY_ENABLED is ignored: ai-bridge forwards to %s, which is this gateway, so relay requests would loop", upstream))
		} else {
			add(envAIBridgeServiceURL, "ai-bridge relay", rewriteRelayPath)
		}
	}
	if len(rules) == 0 {
		return
	}

	router.Use(func(c *gin.Context) {
		for _, rule := range rules {
			targetPath, matched := rule.rewrite(c.Request.URL.Path)
			if !matched {
				continue
			}
			c.Request.URL.Path = targetPath
			c.Request.URL.RawPath = ""
			rule.proxy.ServeHTTP(c.Writer, c.Request)
			c.Abort()
			return
		}
	})
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

// keepPath forwards the public path unchanged, for services mounted under their
// own root path such as /connect or /ai-bridge.
func keepPath(prefix string) rewriteFunc {
	return func(path string) (string, bool) {
		return path, matchesPrefix(path, prefix)
	}
}

// rewriteInvitationsPath strips /invitations from /invitations/api/* because the
// service serves its API under /api.
func rewriteInvitationsPath(path string) (string, bool) {
	const public = "/invitations/api"
	if !matchesPrefix(path, public) {
		return "", false
	}
	return "/api" + strings.TrimPrefix(path, public), true
}

func rewriteRelayPath(path string) (string, bool) {
	for _, prefix := range relayPathPrefixes {
		if matchesPrefix(path, prefix) {
			return path, true
		}
	}
	// new-api also exposes /:mode/mj, for example /turbo/mj/submit/imagine.
	segments := strings.Split(strings.TrimPrefix(path, "/"), "/")
	if len(segments) >= 2 && segments[0] != "api" && segments[1] == "mj" {
		return path, true
	}
	return "", false
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

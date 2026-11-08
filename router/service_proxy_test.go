package router

import (
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestBundledServicePathOwnership(t *testing.T) {
	for _, testCase := range []struct {
		name    string
		owned   bool
		matcher func(string) bool
		path    string
	}{
		{name: "invitations api root", owned: true, matcher: prefixMatcher(invitationsServicePaths), path: "/invitations/api"},
		{name: "invitations api path", owned: true, matcher: prefixMatcher(invitationsServicePaths), path: "/invitations/api/invitations/me"},
		{name: "invitations page stays on the dashboard", matcher: prefixMatcher(invitationsServicePaths), path: "/invitations"},
		{name: "invitations sibling path", matcher: prefixMatcher(invitationsServicePaths), path: "/invitations/apiary"},

		{name: "connect api root", owned: true, matcher: prefixMatcher(connectServicePaths), path: "/connect/api"},
		{name: "connect api clients", owned: true, matcher: prefixMatcher(connectServicePaths), path: "/connect/api/clients/abc"},
		{name: "connect authorize", owned: true, matcher: prefixMatcher(connectServicePaths), path: "/connect/authorize"},
		{name: "connect consent", owned: true, matcher: prefixMatcher(connectServicePaths), path: "/connect/authorize/consent"},
		{name: "connect token", owned: true, matcher: prefixMatcher(connectServicePaths), path: "/connect/token"},
		{name: "connect userinfo", owned: true, matcher: prefixMatcher(connectServicePaths), path: "/connect/userinfo"},
		{name: "connect discovery", owned: true, matcher: prefixMatcher(connectServicePaths), path: "/connect/.well-known/openid-configuration"},
		{name: "connect jwks", owned: true, matcher: prefixMatcher(connectServicePaths), path: "/connect/.well-known/jwks.json"},
		{name: "connect session exchange", owned: true, matcher: prefixMatcher(connectServicePaths), path: "/connect/auth/exchange"},
		{name: "connect app creation", owned: true, matcher: prefixMatcher(connectServicePaths), path: "/connect/clients"},
		{name: "connect page stays on the dashboard", matcher: prefixMatcher(connectServicePaths), path: "/connect"},
		{name: "connect authorizations page stays on the dashboard", matcher: prefixMatcher(connectServicePaths), path: "/connect/authorizations"},
		{name: "connect dashboard page stays on the dashboard", matcher: prefixMatcher(connectServicePaths), path: "/connect/dashboard"},
		{name: "connect console asset stays on the dashboard", matcher: prefixMatcher(connectServicePaths), path: "/connect/page.js"},
		{name: "connect sibling path", matcher: prefixMatcher(connectServicePaths), path: "/connections"},

		{name: "ai-bridge api root", owned: true, matcher: prefixMatcher(aiBridgeServicePaths), path: "/ai-bridge/api"},
		{name: "ai-bridge snapshot", owned: true, matcher: prefixMatcher(aiBridgeServicePaths), path: "/ai-bridge/api/snapshot"},
		{name: "ai-bridge session exchange", owned: true, matcher: prefixMatcher(aiBridgeServicePaths), path: "/ai-bridge/auth/exchange"},
		{name: "ai-bridge console stays on the dashboard", matcher: prefixMatcher(aiBridgeServicePaths), path: "/ai-bridge"},
		{name: "ai-bridge console asset stays on the dashboard", matcher: prefixMatcher(aiBridgeServicePaths), path: "/ai-bridge/admin.js"},
		{name: "ai-bridge sibling path", matcher: prefixMatcher(aiBridgeServicePaths), path: "/ai-bridge/apiary"},

		{name: "relay v1", owned: true, matcher: relayPathMatcher, path: "/v1/chat/completions"},
		{name: "relay v1beta", owned: true, matcher: relayPathMatcher, path: "/v1beta/models"},
		{name: "relay midjourney", owned: true, matcher: relayPathMatcher, path: "/mj/submit/imagine"},
		{name: "relay midjourney mode", owned: true, matcher: relayPathMatcher, path: "/turbo/mj/submit/imagine"},
		{name: "relay keeps dashboard api", matcher: relayPathMatcher, path: "/api/status"},
		{name: "relay v10 is not v1", matcher: relayPathMatcher, path: "/v10/chat/completions"},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			assert.Equal(t, testCase.owned, testCase.matcher(testCase.path))
		})
	}
}

func newBundledServiceEngine() *gin.Engine {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	registerBundledServices(engine)
	// Stand-in for the embedded dashboard, which owns the remaining paths.
	engine.NoRoute(func(c *gin.Context) {
		c.String(http.StatusOK, "dashboard:"+c.Request.URL.Path)
	})
	return engine
}

func TestBundledServicesForwardRequests(t *testing.T) {
	received := make(chan string, 3)
	service := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		body, _ := io.ReadAll(request.Body)
		received <- request.Method + " " + request.URL.RequestURI() + " " + string(body)
		writer.Header().Set("Content-Type", "application/json")
		_, _ = writer.Write([]byte(`{"success":true}`))
	}))
	t.Cleanup(service.Close)

	t.Setenv(envInvitationsServiceURL, service.URL)
	t.Setenv(envConnectServiceURL, service.URL)
	t.Setenv(envAIBridgeServiceURL, "")

	engine := newBundledServiceEngine()
	gateway := httptest.NewServer(engine)
	t.Cleanup(gateway.Close)

	// invitations: the /invitations prefix is stripped because the service
	// serves its API under /api.
	response, err := http.Post(gateway.URL+"/invitations/api/invitations/withdraw?page=2", "application/json", strings.NewReader(`{"amount":1}`))
	require.NoError(t, err)
	defer response.Body.Close()
	body, err := io.ReadAll(response.Body)
	require.NoError(t, err)
	require.Equal(t, http.StatusOK, response.StatusCode)
	assert.JSONEq(t, `{"success":true}`, string(body))
	assert.Equal(t, "POST /api/invitations/withdraw?page=2 {\"amount\":1}", <-received)

	// connect: the path is forwarded unchanged.
	response, err = http.Get(gateway.URL + "/connect/api/clients")
	require.NoError(t, err)
	defer response.Body.Close()
	require.Equal(t, http.StatusOK, response.StatusCode)
	assert.Equal(t, "GET /connect/api/clients ", <-received)

	// The dashboard keeps the page paths of both services.
	for _, path := range []string{"/connect", "/connect/dashboard", "/ai-bridge"} {
		response, err = http.Get(gateway.URL + path)
		require.NoError(t, err)
		defer response.Body.Close()
		page, err := io.ReadAll(response.Body)
		require.NoError(t, err)
		assert.Equal(t, http.StatusOK, response.StatusCode)
		assert.Equal(t, "dashboard:"+path, string(page))
	}
}

func TestUnconfiguredServiceAnswersJSONNotFound(t *testing.T) {
	t.Setenv(envInvitationsServiceURL, "")
	t.Setenv(envConnectServiceURL, "")
	t.Setenv(envAIBridgeServiceURL, "")

	engine := newBundledServiceEngine()
	gateway := httptest.NewServer(engine)
	t.Cleanup(gateway.Close)

	for _, path := range []string{
		"/invitations/api/invitations/me",
		"/connect/api/clients",
		"/connect/authorize",
		"/ai-bridge/api/snapshot",
	} {
		response, err := http.Get(gateway.URL + path)
		require.NoError(t, err)
		defer response.Body.Close()
		body, err := io.ReadAll(response.Body)
		require.NoError(t, err)
		assert.Equal(t, http.StatusNotFound, response.StatusCode, path)
		assert.Contains(t, string(body), "service is not configured", path)
	}
}

func TestServiceWithInvalidURLIsTreatedAsAbsent(t *testing.T) {
	t.Setenv(envConnectServiceURL, "127.0.0.1:4000")

	engine := newBundledServiceEngine()
	gateway := httptest.NewServer(engine)
	t.Cleanup(gateway.Close)

	response, err := http.Get(gateway.URL + "/connect/api/clients")
	require.NoError(t, err)
	defer response.Body.Close()
	assert.Equal(t, http.StatusNotFound, response.StatusCode)
}

func TestConfiguredServiceReturnsBadGatewayWhenUnreachable(t *testing.T) {
	t.Setenv(envConnectServiceURL, "http://127.0.0.1:1")

	engine := newBundledServiceEngine()
	gateway := httptest.NewServer(engine)
	t.Cleanup(gateway.Close)

	response, err := http.Get(gateway.URL + "/connect/authorize")
	require.NoError(t, err)
	defer response.Body.Close()
	body, err := io.ReadAll(response.Body)
	require.NoError(t, err)
	assert.Equal(t, http.StatusBadGateway, response.StatusCode)
	assert.Equal(t, "connect service is unavailable", string(body))
}

func TestRelayRoutingRefusesToLoopBackIntoTheGateway(t *testing.T) {
	t.Setenv("PORT", "3000")
	t.Setenv(envAIBridgeRelayEnabled, "true")
	t.Setenv(envAIBridgeUpstreamBaseURL, "http://127.0.0.1:3000")
	t.Setenv(envAIBridgeServiceURL, "http://127.0.0.1:1")

	engine := newBundledServiceEngine()
	gateway := httptest.NewServer(engine)
	t.Cleanup(gateway.Close)

	// The relay rule is refused, so relay paths stay on the gateway.
	response, err := http.Post(gateway.URL+"/v1/chat/completions", "application/json", strings.NewReader("{}"))
	require.NoError(t, err)
	defer response.Body.Close()
	assert.Equal(t, http.StatusOK, response.StatusCode)

	body, err := io.ReadAll(response.Body)
	require.NoError(t, err)
	assert.Equal(t, "dashboard:/v1/chat/completions", string(body))
}

func TestRelayRoutingRequiresAConfiguredService(t *testing.T) {
	t.Setenv("PORT", "3000")
	t.Setenv(envAIBridgeRelayEnabled, "true")
	t.Setenv(envAIBridgeServiceURL, "")

	engine := newBundledServiceEngine()
	gateway := httptest.NewServer(engine)
	t.Cleanup(gateway.Close)

	// Without a service the relay switch cannot be honored, and relay paths must
	// keep working on the gateway instead of answering 404.
	response, err := http.Post(gateway.URL+"/v1/chat/completions", "application/json", strings.NewReader("{}"))
	require.NoError(t, err)
	defer response.Body.Close()
	body, err := io.ReadAll(response.Body)
	require.NoError(t, err)
	assert.Equal(t, http.StatusOK, response.StatusCode)
	assert.Equal(t, "dashboard:/v1/chat/completions", string(body))
}

func TestRelayRoutingForwardsWhenAIBridgeHasAnotherUpstream(t *testing.T) {
	service := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.Header().Set("Content-Type", "application/json")
		_, _ = writer.Write([]byte(`{"upstream":"` + request.URL.Path + `"}`))
	}))
	t.Cleanup(service.Close)

	t.Setenv("PORT", "3000")
	t.Setenv(envAIBridgeRelayEnabled, "true")
	t.Setenv(envAIBridgeUpstreamBaseURL, "http://10.0.0.5:3000")
	t.Setenv(envAIBridgeServiceURL, service.URL)

	engine := newBundledServiceEngine()
	gateway := httptest.NewServer(engine)
	t.Cleanup(gateway.Close)

	for _, path := range []string{"/v1/chat/completions", "/turbo/mj/submit/imagine"} {
		response, err := http.Post(gateway.URL+path, "application/json", strings.NewReader("{}"))
		require.NoError(t, err)
		defer response.Body.Close()
		body, err := io.ReadAll(response.Body)
		require.NoError(t, err)
		require.Equal(t, http.StatusOK, response.StatusCode, path)
		assert.JSONEq(t, `{"upstream":"`+path+`"}`, string(body), path)
	}
}

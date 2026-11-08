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

func TestBundledServicePathRewrites(t *testing.T) {
	for _, testCase := range []struct {
		name        string
		rewrite     rewriteFunc
		path        string
		wantPath    string
		wantMatched bool
	}{
		{name: "invitations api root", rewrite: rewriteInvitationsPath, path: "/invitations/api", wantPath: "/api", wantMatched: true},
		{name: "invitations api path", rewrite: rewriteInvitationsPath, path: "/invitations/api/invitations/me", wantPath: "/api/invitations/me", wantMatched: true},
		{name: "invitations page stays on the gateway", rewrite: rewriteInvitationsPath, path: "/invitations", wantMatched: false},
		{name: "invitations sibling path", rewrite: rewriteInvitationsPath, path: "/invitations/apiary", wantMatched: false},
		{name: "connect root", rewrite: keepPath("/connect"), path: "/connect", wantPath: "/connect", wantMatched: true},
		{name: "connect authorize", rewrite: keepPath("/connect"), path: "/connect/authorize", wantPath: "/connect/authorize", wantMatched: true},
		{name: "connect sibling path", rewrite: keepPath("/connect"), path: "/connections", wantMatched: false},
		{name: "ai-bridge console", rewrite: keepPath("/ai-bridge"), path: "/ai-bridge", wantPath: "/ai-bridge", wantMatched: true},
		{name: "ai-bridge api", rewrite: keepPath("/ai-bridge"), path: "/ai-bridge/api/keys", wantPath: "/ai-bridge/api/keys", wantMatched: true},
		{name: "relay v1 prefix", rewrite: rewriteRelayPath, path: "/v1/chat/completions", wantPath: "/v1/chat/completions", wantMatched: true},
		{name: "relay v10 is not v1", rewrite: rewriteRelayPath, path: "/v10/chat/completions", wantMatched: false},
		{name: "relay midjourney mode", rewrite: rewriteRelayPath, path: "/turbo/mj/submit/imagine", wantPath: "/turbo/mj/submit/imagine", wantMatched: true},
		{name: "relay skips dashboard api", rewrite: rewriteRelayPath, path: "/api/mj/task", wantMatched: false},
		{name: "relay leaves dashboard routes", rewrite: rewriteRelayPath, path: "/api/status", wantMatched: false},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			path, matched := testCase.rewrite(testCase.path)
			assert.Equal(t, testCase.wantMatched, matched)
			if testCase.wantMatched {
				assert.Equal(t, testCase.wantPath, path)
			}
		})
	}
}

func TestBundledServicesForwardRequestsAndFallThrough(t *testing.T) {
	gin.SetMode(gin.TestMode)
	received := make(chan string, 1)
	service := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		body, _ := io.ReadAll(request.Body)
		received <- request.Method + " " + request.URL.RequestURI() + " " + string(body)
		writer.Header().Set("Content-Type", "application/json")
		_, _ = writer.Write([]byte(`{"success":true}`))
	}))
	t.Cleanup(service.Close)

	t.Setenv(envInvitationsServiceURL, service.URL)

	engine := gin.New()
	registerBundledServices(engine)
	engine.GET("/invitations", func(c *gin.Context) { c.String(http.StatusOK, "dashboard") })
	gateway := httptest.NewServer(engine)
	t.Cleanup(gateway.Close)

	response, err := http.Post(gateway.URL+"/invitations/api/invitations/withdraw?page=2", "application/json", strings.NewReader(`{"amount":1}`))
	require.NoError(t, err)
	defer response.Body.Close()
	body, err := io.ReadAll(response.Body)
	require.NoError(t, err)

	require.Equal(t, http.StatusOK, response.StatusCode)
	assert.JSONEq(t, `{"success":true}`, string(body))
	assert.Equal(t, "POST /api/invitations/withdraw?page=2 {\"amount\":1}", <-received)

	// Paths owned by the gateway keep working when a service is configured.
	response, err = http.Get(gateway.URL + "/invitations")
	require.NoError(t, err)
	defer response.Body.Close()
	body, err = io.ReadAll(response.Body)
	require.NoError(t, err)
	assert.Equal(t, http.StatusOK, response.StatusCode)
	assert.Equal(t, "dashboard", string(body))
}
func TestUnconfiguredServiceKeepsGatewayRouting(t *testing.T) {
	gin.SetMode(gin.TestMode)
	t.Setenv(envInvitationsServiceURL, "")
	t.Setenv(envConnectServiceURL, "")
	t.Setenv(envAIBridgeServiceURL, "")

	engine := gin.New()
	registerBundledServices(engine)
	engine.GET("/invitations", func(c *gin.Context) { c.String(http.StatusOK, "dashboard") })
	gateway := httptest.NewServer(engine)
	t.Cleanup(gateway.Close)

	response, err := http.Get(gateway.URL + "/invitations")
	require.NoError(t, err)
	defer response.Body.Close()
	assert.Equal(t, http.StatusOK, response.StatusCode)

	response, err = http.Get(gateway.URL + "/invitations/api/invitations/me")
	require.NoError(t, err)
	defer response.Body.Close()
	assert.Equal(t, http.StatusNotFound, response.StatusCode)
}

func TestConfiguredServiceReturnsBadGatewayWhenUnreachable(t *testing.T) {
	gin.SetMode(gin.TestMode)
	t.Setenv(envConnectServiceURL, "http://127.0.0.1:1")

	engine := gin.New()
	registerBundledServices(engine)
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
	gin.SetMode(gin.TestMode)
	t.Setenv("PORT", "3000")
	t.Setenv(envAIBridgeRelayEnabled, "true")
	t.Setenv(envAIBridgeUpstreamBaseURL, "http://127.0.0.1:3000")
	t.Setenv(envAIBridgeServiceURL, "http://127.0.0.1:1")

	engine := gin.New()
	registerBundledServices(engine)
	gateway := httptest.NewServer(engine)
	t.Cleanup(gateway.Close)

	// The relay rule is refused, so the request is not proxied at all.
	response, err := http.Post(gateway.URL+"/v1/chat/completions", "application/json", strings.NewReader("{}"))
	require.NoError(t, err)
	defer response.Body.Close()
	assert.Equal(t, http.StatusNotFound, response.StatusCode)
}

func TestRelayRoutingForwardsWhenAIBridgeHasAnotherUpstream(t *testing.T) {
	gin.SetMode(gin.TestMode)
	service := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.Header().Set("Content-Type", "application/json")
		_, _ = writer.Write([]byte(`{"upstream":"` + request.URL.Path + `"}`))
	}))
	t.Cleanup(service.Close)

	t.Setenv("PORT", "3000")
	t.Setenv(envAIBridgeRelayEnabled, "true")
	t.Setenv(envAIBridgeUpstreamBaseURL, "http://10.0.0.5:3000")
	t.Setenv(envAIBridgeServiceURL, service.URL)

	engine := gin.New()
	registerBundledServices(engine)
	gateway := httptest.NewServer(engine)
	t.Cleanup(gateway.Close)

	response, err := http.Post(gateway.URL+"/v1/chat/completions", "application/json", strings.NewReader("{}"))
	require.NoError(t, err)
	defer response.Body.Close()
	body, err := io.ReadAll(response.Body)
	require.NoError(t, err)
	require.Equal(t, http.StatusOK, response.StatusCode)
	assert.JSONEq(t, `{"upstream":"/v1/chat/completions"}`, string(body))
}

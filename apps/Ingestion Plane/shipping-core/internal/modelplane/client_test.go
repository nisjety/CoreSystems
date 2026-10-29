package modelplane

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func newTestServers(t *testing.T, invokeHandler http.HandlerFunc) (authCore, gateway *httptest.Server) {
	t.Helper()
	authCore = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/model-plane/internal-token" {
			t.Errorf("unexpected auth-core path %s", r.URL.Path)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "test-key" {
			t.Errorf("X-Internal-Api-Key = %q, want test-key", got)
		}
		var body internalTokenRequest
		_ = json.NewDecoder(r.Body).Decode(&body)
		if body.OrgID == "" || body.UserID == "" {
			t.Errorf("token request missing orgId/userId: %+v", body)
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(internalTokenResponse{Token: "test-jwt", ExpiresInSeconds: 300})
	}))
	gateway = httptest.NewServer(invokeHandler)
	return authCore, gateway
}

func TestClient_Invoke_MintsTokenAndCallsGateway(t *testing.T) {
	var gotAuth string
	authCore, gateway := newTestServers(t, func(w http.ResponseWriter, r *http.Request) {
		gotAuth = r.Header.Get("Authorization")
		if r.URL.Path != "/v1/invoke" {
			t.Errorf("unexpected gateway path %s", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(InvokeResponse{RequestID: "req_1", Content: `{"ok":true}`, ModelUsed: "verevon-balance"})
	})
	defer authCore.Close()
	defer gateway.Close()

	c := New(Config{AuthCoreURL: authCore.URL, ModelGatewayURL: gateway.URL, InternalAPIKey: "test-key"})
	resp, err := c.Invoke(context.Background(), InvokeRequest{Content: "compare these quotes", Model: "verevon-balance"})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if gotAuth != "Bearer test-jwt" {
		t.Errorf("Authorization = %q, want Bearer test-jwt", gotAuth)
	}
	if resp.Content != `{"ok":true}` {
		t.Errorf("Content = %q", resp.Content)
	}
}

func TestClient_Invoke_DefaultsToSystemIdentityWhenUnset(t *testing.T) {
	var gotOrg, gotUser string
	mux := http.NewServeMux()
	mux.HandleFunc("/api/model-plane/internal-token", func(w http.ResponseWriter, r *http.Request) {
		var body internalTokenRequest
		_ = json.NewDecoder(r.Body).Decode(&body)
		gotOrg, gotUser = body.OrgID, body.UserID
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(internalTokenResponse{Token: "test-jwt", ExpiresInSeconds: 300})
	})
	authCore := httptest.NewServer(mux)
	gateway := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(InvokeResponse{Content: "{}"})
	}))
	defer authCore.Close()
	defer gateway.Close()

	c := New(Config{AuthCoreURL: authCore.URL, ModelGatewayURL: gateway.URL, InternalAPIKey: "test-key"})
	if _, err := c.Invoke(context.Background(), InvokeRequest{Content: "x"}); err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if gotOrg != SystemOrgID || gotUser != SystemUserID {
		t.Errorf("got org=%q user=%q, want system identity", gotOrg, gotUser)
	}
}

func TestClient_Invoke_CachesTokenAcrossCalls(t *testing.T) {
	tokenRequests := 0
	mux := http.NewServeMux()
	mux.HandleFunc("/api/model-plane/internal-token", func(w http.ResponseWriter, r *http.Request) {
		tokenRequests++
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(internalTokenResponse{Token: "test-jwt", ExpiresInSeconds: 300})
	})
	authCore := httptest.NewServer(mux)
	gateway := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(InvokeResponse{Content: "{}"})
	}))
	defer authCore.Close()
	defer gateway.Close()

	c := New(Config{AuthCoreURL: authCore.URL, ModelGatewayURL: gateway.URL, InternalAPIKey: "test-key"})
	for range 3 {
		if _, err := c.Invoke(context.Background(), InvokeRequest{Content: "x"}); err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
	}
	if tokenRequests != 1 {
		t.Errorf("token minted %d times, want 1 (cached)", tokenRequests)
	}
}

func TestClient_Invoke_NotConfiguredFailsFast(t *testing.T) {
	c := New(Config{})
	_, err := c.Invoke(context.Background(), InvokeRequest{Content: "x"})
	if err == nil || !strings.Contains(err.Error(), "not configured") {
		t.Fatalf("got err=%v, want a not-configured error", err)
	}
}

func TestClient_Invoke_GatewayErrorSurfaced(t *testing.T) {
	authCore, gateway := newTestServers(t, func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusBadGateway)
		_, _ = w.Write([]byte(`{"error":"inference-core unreachable"}`))
	})
	defer authCore.Close()
	defer gateway.Close()

	c := New(Config{AuthCoreURL: authCore.URL, ModelGatewayURL: gateway.URL, InternalAPIKey: "test-key"})
	_, err := c.Invoke(context.Background(), InvokeRequest{Content: "x"})
	if err == nil {
		t.Fatal("expected an error for a 502 gateway response")
	}
}

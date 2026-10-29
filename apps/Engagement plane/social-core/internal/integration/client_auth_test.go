package integration

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/social"
)

func TestTenantCallsUseScopedBearerOnly(t *testing.T) {
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		if got := r.Header.Get("Authorization"); got != "Bearer scoped-test-token" {
			t.Errorf("Authorization = %q", got)
		}
		if got := r.Header.Get("X-Internal-API-Key"); got != "" {
			t.Error("tenant request carried an internal API key")
		}
		w.Header().Set("Content-Type", "application/json")
		if strings.HasSuffix(r.URL.Path, "/connections") {
			_, _ = w.Write([]byte(`{"success":true,"data":{"connections":[]}}`))
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": map[string]any{"action": map[string]any{"providerKey": "meta", "operation": "meta.ads.insights", "result": map[string]any{}}}})
	}))
	defer server.Close()
	client := NewClient(Config{BaseURL: server.URL, TenantBearerToken: "scoped-test-token", TenantOrganizationID: "org-1"})
	if _, err := client.ListSocialAccounts(context.Background(), "org-1"); err != nil {
		t.Fatal(err)
	}
	if _, err := client.ExecuteAction(context.Background(), social.ActionRequest{ConnectionID: "conn-1", Operation: "meta.ads.insights"}); err != nil {
		t.Fatal(err)
	}
	if calls != 2 {
		t.Fatalf("requests = %d, want 2", calls)
	}
}

func TestTenantCallsFailClosedWithoutBearerOrMatchingOrganization(t *testing.T) {
	serverCalls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { serverCalls++ }))
	defer server.Close()
	for _, cfg := range []Config{
		{BaseURL: server.URL, TenantOrganizationID: "org-1"},
		{BaseURL: server.URL, TenantBearerToken: "token", TenantOrganizationID: "org-1"},
	} {
		client := NewClient(cfg)
		if cfg.TenantBearerToken == "" {
			if _, err := client.ListSocialAccounts(context.Background(), "org-1"); err == nil {
				t.Fatal("missing bearer must fail")
			}
			if _, err := client.ExecuteAction(context.Background(), social.ActionRequest{ConnectionID: "conn-1", Operation: "meta.ads.insights"}); err == nil {
				t.Fatal("missing bearer must fail action")
			}
		} else {
			if _, err := client.ListSocialAccounts(context.Background(), "org-2"); err == nil {
				t.Fatal("cross-org listing must fail")
			}
		}
	}
	if serverCalls != 0 {
		t.Fatalf("unexpected outbound requests: %d", serverCalls)
	}
}

func TestListSocialAccountsIgnoresOtherOrganizationInGatewayResponse(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"success":true,"data":{"connections":[{"id":"other","providerKey":"meta","organizationId":"org-2"},{"id":"own","providerKey":"meta","organizationId":"org-1"}]}}`))
	}))
	defer server.Close()
	client := NewClient(Config{BaseURL: server.URL, TenantBearerToken: "token", TenantOrganizationID: "org-1"})
	accounts, err := client.ListSocialAccounts(context.Background(), "org-1")
	if err != nil {
		t.Fatal(err)
	}
	if len(accounts) != 1 || accounts[0].ConnectionID != "own" {
		t.Fatalf("accounts = %+v", accounts)
	}
}

func TestProviderAndAccountNormalization(t *testing.T) {
	for input, want := range map[string]string{
		" Twitter ": "x", "linked_in": "linkedin", "tik-tok": "tiktok",
		"facebook-page": "facebook", "meta-business": "meta", "meta_ads": "meta-ads",
		"whatsapp-cloud": "whatsapp", "snap": "snapchat", " LinkedIn ": "linkedin",
	} {
		if got := normalizeProvider(input); got != want {
			t.Errorf("normalizeProvider(%q) = %q, want %q", input, got, want)
		}
	}
	for input, want := range map[string]string{
		"active": social.AccountStatusConnected, "expired": social.AccountStatusExpired,
		"error": social.AccountStatusError, "disconnected": social.AccountStatusDisconnected,
	} {
		if got := accountStatus(input); got != want {
			t.Errorf("accountStatus(%q) = %q", input, got)
		}
	}
	if got := tokenState("active", time.Now().Add(-time.Minute)); got != "expired" {
		t.Errorf("expired state = %q", got)
	}
	if got := tokenState("disconnected", time.Time{}); got != "missing" {
		t.Errorf("missing state = %q", got)
	}
	if got := tokenExpiresAt(time.Time{}); got != nil {
		t.Fatal("zero expiry must be nil")
	}
}

func TestTenantGatewayErrorsRemainErrors(t *testing.T) {
	for _, response := range []struct {
		status int
		body   string
	}{
		{http.StatusUnauthorized, `{"success":false,"error":{"code":"unauthorized","message":"denied"}}`},
		{http.StatusOK, `{"success":false,"error":{"code":"denied","message":"not allowed"}}`},
		{http.StatusBadGateway, `{"success":false}`},
		{http.StatusOK, `not-json`},
		{http.StatusOK, `{"success":true,"data":"bad-shape"}`},
	} {
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			w.WriteHeader(response.status)
			_, _ = w.Write([]byte(response.body))
		}))
		client := NewClient(Config{BaseURL: server.URL, TenantBearerToken: "token", TenantOrganizationID: "org-1"})
		if _, err := client.ListSocialAccounts(context.Background(), "org-1"); err == nil {
			t.Errorf("listing must fail for %d: %s", response.status, response.body)
		}
		server.Close()
	}
}

func TestIntegrationBaseURLAllowsOnlySecureOrApprovedLocalOrigin(t *testing.T) {
	for _, value := range []string{"https://integration.example", "http://integration-api:3026", "http://127.0.0.1:3026", "http://[::1]:3026"} {
		if err := validateIntegrationBaseURL(value); err != nil {
			t.Errorf("approved %q: %v", value, err)
		}
	}
	for _, value := range []string{
		"http://evil.example", "http://integration-api.evil.example", "http://127.0.0.1.evil.example",
		"http://user:pass@integration-api:3026", "https://user@integration.example",
		"http://integration-api:3026/extra", "http://integration-api:3026?x=1",
		"http://integration-api:3026#fragment", "ftp://integration-api:3026", "//integration-api:3026",
	} {
		if err := validateIntegrationBaseURL(value); err == nil {
			t.Errorf("unsafe %q was accepted", value)
		}
	}
}

func TestTenantClientNeverFollowsBearerRedirect(t *testing.T) {
	redirected := 0
	receiver := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { redirected++ }))
	defer receiver.Close()
	entry := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, receiver.URL+"/receive", http.StatusTemporaryRedirect)
	}))
	defer entry.Close()
	client := NewClient(Config{BaseURL: entry.URL, TenantBearerToken: "secret-test-bearer", TenantOrganizationID: "org-1", HTTPClient: &http.Client{}})
	if _, err := client.ListSocialAccounts(context.Background(), "org-1"); err == nil {
		t.Fatal("redirect must not be treated as success")
	}
	if redirected != 0 {
		t.Fatalf("bearer request followed redirect %d times", redirected)
	}
}

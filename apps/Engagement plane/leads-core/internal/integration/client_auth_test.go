package integration

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestTenantCallsUseScopedBearerOnly(t *testing.T) {
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		if got := r.Header.Get("Authorization"); got != "Bearer scoped-test-token" {
			t.Errorf("Authorization = %q", got)
		}
		if got := r.Header.Get("X-Internal-API-Key"); got != "" {
			t.Error("tenant request carried internal API key")
		}
		if r.URL.Path == "/api/v1/connections" {
			if got := r.URL.Query().Get("organizationId"); got != "org-1" {
				t.Errorf("organizationId = %q", got)
			}
			_, _ = w.Write([]byte(`{"success":true,"data":{"connections":[]}}`))
			return
		}
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{}}}}`))
	}))
	defer server.Close()
	client := NewClient(Config{BaseURL: server.URL, TenantBearerToken: "scoped-test-token", TenantOrganizationID: "org-1"})
	if _, err := client.ListConnections(context.Background(), "", "linkedin"); err != nil {
		t.Fatal(err)
	}
	if _, err := client.ExecuteAction(context.Background(), "conn-1", "linkedin.lead.forms", nil); err != nil {
		t.Fatal(err)
	}
	if calls != 2 {
		t.Fatalf("requests = %d, want 2", calls)
	}
}

func TestTenantCallsFailClosedWithoutBearerOrMatchingOrganization(t *testing.T) {
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls++ }))
	defer server.Close()
	client := NewClient(Config{BaseURL: server.URL, TenantOrganizationID: "org-1"})
	if _, err := client.ListConnections(context.Background(), "", "linkedin"); err == nil {
		t.Fatal("missing bearer must fail listing")
	}
	if _, err := client.ExecuteAction(context.Background(), "conn-1", "linkedin.lead.forms", nil); err == nil {
		t.Fatal("missing bearer must fail action")
	}
	client = NewClient(Config{BaseURL: server.URL, TenantBearerToken: "token", TenantOrganizationID: "org-1"})
	if _, err := client.ListConnections(context.Background(), "org-2", "linkedin"); err == nil {
		t.Fatal("cross-org listing must fail")
	}
	if calls != 0 {
		t.Fatalf("unexpected outbound requests: %d", calls)
	}
}

func TestActionsRejectMissingConnectionOrOperationBeforeGatewayCall(t *testing.T) {
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls++ }))
	defer server.Close()
	client := NewClient(Config{BaseURL: server.URL, TenantBearerToken: "scoped-test-token", TenantOrganizationID: "org-1"})
	for _, tc := range []struct{ connection, operation string }{
		{"", "linkedin.lead.forms"}, {" \t", "linkedin.lead.forms"}, {"conn-1", ""}, {"conn-1", " \t"},
	} {
		if _, err := client.ExecuteAction(context.Background(), tc.connection, tc.operation, nil); err == nil {
			t.Errorf("missing action input must be rejected: %+v", tc)
		}
	}
	if calls != 0 {
		t.Fatalf("invalid action made %d gateway requests", calls)
	}
}

func TestListConnectionsIgnoresOtherOrganizationInGatewayResponse(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"success":true,"data":{"connections":[{"id":"other","organizationId":"org-2"},{"id":"own","organizationId":"org-1"}]}}`))
	}))
	defer server.Close()
	client := NewClient(Config{BaseURL: server.URL, TenantBearerToken: "token", TenantOrganizationID: "org-1"})
	connections, err := client.ListConnections(context.Background(), "", "linkedin")
	if err != nil {
		t.Fatal(err)
	}
	if len(connections) != 1 || connections[0].ID != "own" {
		t.Fatalf("connections = %+v", connections)
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
	if _, err := client.ListConnections(context.Background(), "", "linkedin"); err == nil {
		t.Fatal("redirect must not be treated as success")
	}
	if redirected != 0 {
		t.Fatalf("bearer request followed redirect %d times", redirected)
	}
}

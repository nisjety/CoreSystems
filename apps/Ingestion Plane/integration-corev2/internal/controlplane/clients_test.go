package controlplane

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/triodelab/integration-corev2/internal/config"
)

func TestAuthClientVerifyTokenUsesAuthCoreInternalContract(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/internal/sessions/verify" {
			t.Fatalf("path = %s, want /internal/sessions/verify", r.URL.Path)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "internal-key" {
			t.Fatalf("internal key = %q, want internal-key", got)
		}
		var body map[string]string
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		if body["token"] != "bearer-token" {
			t.Fatalf("token = %q, want bearer-token", body["token"])
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"success": true,
			"data": map[string]any{
				"userId":         "user-1",
				"organizationId": "org-1",
				"workspaceId":    "workspace-1",
				"role":           "admin",
				"email":          "a@example.com",
			},
		})
	}))
	defer server.Close()

	client := NewAuthClient(testControlPlaneConfig(server.URL), server.Client())
	principal, err := client.VerifyToken(t.Context(), "bearer-token")
	if err != nil {
		t.Fatalf("VerifyToken error: %v", err)
	}
	if principal.UserID != "user-1" || principal.OrganizationID != "org-1" || principal.Role != "admin" {
		t.Fatalf("principal = %#v", principal)
	}
}

func TestOrgClientReadsOrgPlanFromOrgCore(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/orgs/org-1" {
			t.Fatalf("path = %s, want /orgs/org-1", r.URL.Path)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "internal-key" {
			t.Fatalf("internal key = %q, want internal-key", got)
		}
		if got := r.Header.Get("X-User-ID"); got != "user-1" {
			t.Fatalf("user header = %q, want user-1", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"id":   "org-1",
			"plan": "advanced",
			"entitlements": []map[string]any{
				{"key": "integrations", "enabled": true},
			},
			"quotas": []map[string]any{
				{"key": "sources", "value": 1, "limit": 10, "reset_period": "month"},
			},
		})
	}))
	defer server.Close()

	cfg := testControlPlaneConfig("http://unused")
	cfg.OrgCoreURL = server.URL
	client := NewOrgClient(cfg, server.Client())
	plan, err := client.GetOrgPlan(t.Context(), "org-1", "user-1")
	if err != nil {
		t.Fatalf("GetOrgPlan error: %v", err)
	}
	if plan.Plan != "pro" || !plan.Entitlements["integrations"] || plan.Quotas["sources"].Limit != 10 {
		t.Fatalf("plan = %#v", plan)
	}
}

func TestBillingClientRecordsUsageThroughBillingCore(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/billing/orgs/org-1/usage" {
			t.Fatalf("path = %s, want /api/v1/billing/orgs/org-1/usage", r.URL.Path)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "internal-key" {
			t.Fatalf("internal key = %q, want internal-key", got)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		if body["metric"] != "connect_session_created" || body["source"] != "integration-corev2" {
			t.Fatalf("body = %#v", body)
		}
		w.WriteHeader(http.StatusAccepted)
		_ = json.NewEncoder(w).Encode(map[string]string{"status": "usage recorded"})
	}))
	defer server.Close()

	cfg := testControlPlaneConfig("http://unused")
	cfg.BillingCoreURL = server.URL
	client := NewBillingClient(cfg, server.Client())
	if err := client.RecordUsage(t.Context(), "org-1", UsageEvent{Metric: "connect_session_created", Quantity: 1}); err != nil {
		t.Fatalf("RecordUsage error: %v", err)
	}
}

func TestAuditClientRecordsAuditThroughAuditCore(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/audit" {
			t.Fatalf("path = %s, want /v1/audit", r.URL.Path)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "internal-key" {
			t.Fatalf("internal key = %q, want internal-key", got)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		if body["org_id"] != "org-1" || body["plane"] != "integration-corev2" || body["event"] != "connection.action.executed" {
			t.Fatalf("body = %#v", body)
		}
		details, ok := body["details"].(map[string]any)
		if !ok || details["providerKey"] != "slack" {
			t.Fatalf("details = %#v, want provider key", body["details"])
		}
		w.WriteHeader(http.StatusAccepted)
	}))
	defer server.Close()

	cfg := testControlPlaneConfig("http://unused")
	cfg.AuditCoreURL = server.URL
	client := NewAuditClient(cfg, server.Client())
	if err := client.RecordAudit(t.Context(), AuditEvent{
		OrgID:      "org-1",
		UserID:     "user-1",
		Event:      "connection.action.executed",
		ResourceID: "conn-1",
		Details:    map[string]any{"providerKey": "slack"},
	}); err != nil {
		t.Fatalf("RecordAudit error: %v", err)
	}
}

func testControlPlaneConfig(authCoreURL string) config.Config {
	return config.Config{
		ServiceName:            "integration-corev2",
		InternalAPIKey:         "fallback-key",
		AuthCoreURL:            authCoreURL,
		AuthCoreInternalAPIKey: "internal-key",
		OrgCoreURL:             "http://org-core",
		BillingCoreURL:         "http://billing-core",
		AuditCoreURL:           "http://audit-core",
	}
}

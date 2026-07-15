package clients

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestBillingClientUsesSessionScopedServicePrincipal(t *testing.T) {
	const token = "session-to-billing-test-secret-at-least-32-bytes"
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.Header.Get("X-Service-Id"); got != "session-core" {
			t.Fatalf("X-Service-Id = %q", got)
		}
		if got := r.Header.Get("X-Service-Token"); got != token {
			t.Fatalf("X-Service-Token = %q", got)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "" {
			t.Fatalf("legacy key leaked: %q", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"orgId":  "org1",
			"plan":   "free",
			"status": "active",
		})
	}))
	defer ts.Close()

	client := NewBillingClient(ts.URL, token)
	account, err := client.GetAccount(context.Background(), "org1")
	if err != nil {
		t.Fatalf("GetAccount: %v", err)
	}
	if account == nil || account.OrgID != "org1" {
		t.Fatalf("account = %#v", account)
	}
}

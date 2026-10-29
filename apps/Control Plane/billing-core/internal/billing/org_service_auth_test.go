package billing

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
)

func TestFetchOrgCoreSeedDoesNotForwardScopedTokenAcrossRedirect(t *testing.T) {
	var redirected atomic.Int32
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		redirected.Add(1)
		w.WriteHeader(http.StatusOK)
	}))
	defer target.Close()
	redirector := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, target.URL, http.StatusTemporaryRedirect)
	}))
	defer redirector.Close()

	t.Setenv("ORG_SERVICE_URL", redirector.URL)
	t.Setenv("ORG_CORE_SERVICE_TOKEN", "billing-org-redirect-secret-at-least-32-bytes")
	NewService(nil, nil, nil).fetchOrgCoreSeed(context.Background(), "org-1")
	if calls := redirected.Load(); calls != 0 {
		t.Fatalf("scoped credential followed %d redirect(s)", calls)
	}
}

func TestFetchOrgCoreSeedUsesBillingScopedPrincipalWithoutLegacyKey(t *testing.T) {
	const token = "billing-to-org-test-secret-at-least-32-bytes"
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		if got := r.Header.Get("X-Service-Id"); got != "billing-core" {
			t.Fatalf("X-Service-Id = %q", got)
		}
		if got := r.Header.Get("X-Service-Token"); got != token {
			t.Fatalf("X-Service-Token = %q", got)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "" {
			t.Fatalf("legacy key leaked: %q", got)
		}
		w.Header().Set("Content-Type", "application/json")
		if r.URL.Path == "/orgs/org-1" {
			_ = json.NewEncoder(w).Encode(map[string]any{"name": "Acme", "plan": "pro"})
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"entitlements": []any{}})
	}))
	defer server.Close()

	t.Setenv("ORG_SERVICE_URL", server.URL)
	t.Setenv("ORG_CORE_SERVICE_TOKEN", token)
	t.Setenv("INTERNAL_API_KEY", "legacy-shared-key")
	service := NewService(nil, nil, nil)
	service.fetchOrgCoreSeed(context.Background(), "org-1")
	if got := calls.Load(); got != 2 {
		t.Fatalf("calls = %d, want 2", got)
	}
}

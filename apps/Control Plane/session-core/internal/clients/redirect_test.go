package clients

import (
	"context"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
)

func TestScopedControlClientsNeverFollowRedirects(t *testing.T) {
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

	org := NewOrgClient(redirector.URL, "session-org-redirect-secret-at-least-32-bytes")
	_, _ = org.GetOrganization(context.Background(), "org-1")
	billing := NewBillingClient(redirector.URL, "session-billing-redirect-secret-at-least-32-bytes")
	_, _ = billing.GetAccount(context.Background(), "org-1")
	if calls := redirected.Load(); calls != 0 {
		t.Fatalf("scoped credentials followed %d redirect(s)", calls)
	}
}

package jobs

import (
	"errors"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/authctx"
)

type executorTestVerifier struct{}

func (executorTestVerifier) Verify(token string) (*authctx.Claims, error) {
	if token != "verified-token" {
		return nil, errors.New("invalid token")
	}
	claims := &authctx.Claims{OrgID: "org_authorized", UserID: "user_authorized", Verified: true}
	claims.Subject = claims.UserID
	return claims, nil
}

func TestFetchLintItemsForwardsVerifiedBearerAndClaimOrg(t *testing.T) {
	var gotAuthorization, gotOrg string
	downstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotAuthorization = r.Header.Get("Authorization")
		gotOrg = r.Header.Get("X-Org-ID")
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"issues":[]}`))
	}))
	t.Cleanup(downstream.Close)

	var callErr error
	guarded := authctx.Middleware(executorTestVerifier{})(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, callErr = fetchLintItems(r.Context(), downstream.URL, "org_authorized")
		w.WriteHeader(http.StatusNoContent)
	}))
	request := httptest.NewRequest(http.MethodGet, "/run", nil)
	request.Header.Set("Authorization", "Bearer verified-token")
	guarded.ServeHTTP(httptest.NewRecorder(), request)

	if callErr != nil {
		t.Fatalf("fetchLintItems: %v", callErr)
	}
	if gotAuthorization != "Bearer verified-token" || gotOrg != "org_authorized" {
		t.Fatalf("forwarded headers = (%q, %q)", gotAuthorization, gotOrg)
	}
}

func TestPostSweepForwardsVerifiedBearerAndClaimOrg(t *testing.T) {
	var gotAuthorization, gotOrg string
	downstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotAuthorization = r.Header.Get("Authorization")
		gotOrg = r.Header.Get("X-Org-ID")
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"accepted":1,"rejected":0}`))
	}))
	t.Cleanup(downstream.Close)

	var callErr error
	guarded := authctx.Middleware(executorTestVerifier{})(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _, callErr = postSweep(r.Context(), downstream.URL, "org_authorized", []sweepItem{{PageID: "page-1", Kind: "stale_wiki"}})
		w.WriteHeader(http.StatusNoContent)
	}))
	request := httptest.NewRequest(http.MethodGet, "/run", nil)
	request.Header.Set("Authorization", "Bearer verified-token")
	guarded.ServeHTTP(httptest.NewRecorder(), request)

	if callErr != nil {
		t.Fatalf("postSweep: %v", callErr)
	}
	if gotAuthorization != "Bearer verified-token" || gotOrg != "org_authorized" {
		t.Fatalf("forwarded headers = (%q, %q)", gotAuthorization, gotOrg)
	}
}

func TestOutboundCallbacksRejectTenantMismatchBeforeNetwork(t *testing.T) {
	var requests atomic.Int32
	downstream := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		requests.Add(1)
	}))
	t.Cleanup(downstream.Close)

	var callErr error
	guarded := authctx.Middleware(executorTestVerifier{})(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, callErr = fetchLintItems(r.Context(), downstream.URL, "org_spoofed")
		w.WriteHeader(http.StatusNoContent)
	}))
	request := httptest.NewRequest(http.MethodGet, "/run", nil)
	request.Header.Set("Authorization", "Bearer verified-token")
	guarded.ServeHTTP(httptest.NewRecorder(), request)

	if callErr == nil {
		t.Fatal("expected tenant mismatch to fail")
	}
	if got := requests.Load(); got != 0 {
		t.Fatalf("downstream requests = %d, want 0", got)
	}
}

package clients

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
)

// browserSiteEraseTestToken is a low-entropy fixture, not a credential.
const browserSiteEraseTestToken = "fixture-user-core-token-aaaaaaaaaaaaaaaaaaaaaaaa"

// TestEraseBrowserSiteChoicesCallsTheInternalRouteWithTheServiceHeaders pins the wire
// contract the erasure saga relies on: one DELETE to the org-core member route, no body,
// and the service principal and token headers that org-core's service auth reads.
func TestEraseBrowserSiteChoicesCallsTheInternalRouteWithTheServiceHeaders(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		if r.Method != http.MethodDelete {
			t.Errorf("method = %s, want DELETE", r.Method)
		}
		if r.URL.Path != "/internal/orgs/org-1/members/user-1/browser-sites" {
			t.Errorf("path = %s, want the org-core member browser-sites route", r.URL.Path)
		}
		if got := r.Header.Get("X-Service-Id"); got != "user-core" {
			t.Errorf("X-Service-Id = %q, want user-core", got)
		}
		if got := r.Header.Get("X-Service-Token"); got != browserSiteEraseTestToken {
			t.Errorf("X-Service-Token did not carry the configured token")
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"ok":true,"prefs_deleted":1,"history_redacted":2}`))
	}))
	defer server.Close()

	client := NewOrgCoreClient(server.URL, "user-core", browserSiteEraseTestToken)
	if client == nil {
		t.Fatal("a configured client must not be nil")
	}
	if err := client.EraseBrowserSiteChoices(context.Background(), "org-1", "user-1"); err != nil {
		t.Fatalf("erase: %v", err)
	}
	if n := calls.Load(); n != 1 {
		t.Fatalf("org-core calls = %d, want 1", n)
	}
}

// TestEraseBrowserSiteChoicesAnswersOnlyOKAndNeverNamesAnID pins that anything but a 200 is
// an error that names the status and no organisation or user id, because the saga stores the
// message as the fan-out's retry record.
func TestEraseBrowserSiteChoicesAnswersOnlyOKAndNeverNamesAnID(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusServiceUnavailable)
	}))
	defer server.Close()

	client := NewOrgCoreClient(server.URL, "user-core", browserSiteEraseTestToken)
	err := client.EraseBrowserSiteChoices(context.Background(), "org-private-1", "user-private-1")
	if err == nil || !strings.Contains(err.Error(), "status 503") {
		t.Fatalf("a 503 must be refused with its status, got %v", err)
	}
	assertNoBrowserSiteIDs(t, err)
}

// TestEraseBrowserSiteChoicesTransportFailureKeepsNoIDs pins that a refused connection, whose
// url.Error names the request URL with both ids, is reported by its cause only.
func TestEraseBrowserSiteChoicesTransportFailureKeepsNoIDs(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {}))
	baseURL := server.URL
	server.Close()

	client := NewOrgCoreClient(baseURL, "user-core", browserSiteEraseTestToken)
	err := client.EraseBrowserSiteChoices(context.Background(), "org-private-1", "user-private-1")
	if err == nil {
		t.Fatal("a refused connection must be an error")
	}
	if !strings.Contains(err.Error(), "call org-core browser-site erasure endpoint") {
		t.Fatalf("transport failure message = %v", err)
	}
	assertNoBrowserSiteIDs(t, err)
}

// TestEraseBrowserSiteChoicesRefusesBlankIDsAndAnUnconfiguredClientBeforeAnyCall pins that a
// blank id and a nil client never reach org-core.
func TestEraseBrowserSiteChoicesRefusesBlankIDsAndAnUnconfiguredClientBeforeAnyCall(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		calls.Add(1)
		w.WriteHeader(http.StatusOK)
	}))
	defer server.Close()

	client := NewOrgCoreClient(server.URL, "user-core", browserSiteEraseTestToken)
	for _, ids := range [][2]string{{"  ", "user-1"}, {"org-1", "   "}} {
		if err := client.EraseBrowserSiteChoices(context.Background(), ids[0], ids[1]); err == nil {
			t.Fatalf("blank ids %q must be refused", ids)
		}
	}
	var unconfigured *OrgCoreClient
	if err := unconfigured.EraseBrowserSiteChoices(context.Background(), "org-1", "user-1"); err == nil {
		t.Fatal("an unconfigured client must refuse")
	}
	if n := calls.Load(); n != 0 {
		t.Fatalf("refused erasures reached org-core %d times", n)
	}
}

func assertNoBrowserSiteIDs(t *testing.T, err error) {
	t.Helper()
	for _, id := range []string{"org-private-1", "user-private-1"} {
		if strings.Contains(err.Error(), id) {
			t.Fatalf("the error names an id, which the retry record would store: %v", err)
		}
	}
}

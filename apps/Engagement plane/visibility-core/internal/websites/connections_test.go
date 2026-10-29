package websites

import (
	"context"
	"strings"
	"testing"
)

func TestConnectionRefreshReusesGrantAndPersistsCheckpoint(t *testing.T) {
	fp := strings.Repeat("a", 64)
	calls := 0
	active := false
	discover := func(context.Context, string) Evidence {
		calls++
		e := Evidence{ConnectionsFingerprint: fp, SearchConsole: Access{Status: "needs_connection"}, GA4: Access{Status: "requires_consent"}}
		if active {
			e.SearchConsole = Access{Status: "connected", ConnectionID: "shared-workspace-google", Property: "sc-domain:triodelab.no", AccessLevel: "siteFullUser"}
		}
		return e
	}
	s := newTestService(t, discover)
	w, _, err := s.Add(t.Context(), "triodelab.no")
	if err != nil {
		t.Fatal(err)
	}
	originalCalls := calls
	if err = s.RefreshConnections(t.Context(), fp, nil); err != nil || calls != originalCalls {
		t.Fatal("unchanged grant rediscovered provider resources")
	}
	fp = strings.Repeat("b", 64)
	active = true
	if err = s.RefreshConnections(t.Context(), fp, nil); err != nil {
		t.Fatal(err)
	}
	r, err := s.Resolve(w.ID, w.Site)
	if err != nil || r.GSCConnectionID != "shared-workspace-google" {
		t.Fatal("did not reuse the shared Google grant")
	}
	repo, err := Open(s.repo.path, s.OrganizationID())
	if err != nil {
		t.Fatal(err)
	}
	resumed := NewService(repo, discover, s.now)
	prior := calls
	if err = resumed.RefreshConnections(t.Context(), fp, nil); err != nil || calls != prior {
		t.Fatal("restart lost connection checkpoint")
	}
	fp = strings.Repeat("c", 64)
	active = false
	if err = resumed.RefreshConnections(t.Context(), fp, nil); err != nil {
		t.Fatal(err)
	}
	r, _ = resumed.Resolve(w.ID, w.Site)
	if r.GSCConnectionID != "" || r.Website.Integrations.SearchConsole.Status != "needs_connection" {
		t.Fatal("revoked grant retained report binding")
	}
}

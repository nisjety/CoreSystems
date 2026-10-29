package sitesources

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type fakeRegistry struct{ record websites.Record }

func (r *fakeRegistry) Resolve(id, site string) (websites.Record, error) {
	if r.record.Website.ID != id || r.record.Website.Site != site {
		return websites.Record{}, websites.ErrNotFound
	}
	return r.record, nil
}
func (*fakeRegistry) OrganizationID() string { return "org_sites" }

type fakeCollector struct {
	report Report
	calls  int
	err    error
	after  func()
}

func (c *fakeCollector) Collect(context.Context, websites.Record, string) (Report, error) {
	c.calls++
	if c.after != nil {
		c.after()
	}
	return c.report, c.err
}
func fixture(t *testing.T) (*Service, *fakeRegistry, *fakeCollector, string) {
	t.Helper()
	dir := t.TempDir()
	_ = os.Chmod(dir, 0700)
	path := filepath.Join(dir, "site-sources.json")
	now := time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC)
	r := &fakeRegistry{websites.Record{Website: websites.Website{ID: "web_00000000000000000000000000000001", Site: "https://triodelab.no/", Revision: 7, Ownership: websites.Ownership{Status: "google_owner"}}}}
	w := r.record.Website
	c := &fakeCollector{report: Report{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Provider: "algolia", Status: "connected", ObservedAt: stamp(now), Window: period(now), Algolia: &Algolia{IndexName: "triodelab", Region: "de", Analytics: Analytics{Status: "connected", ReportedDays: 7, Searches: integer(10), NoResultSearches: integer(2), NoResultRate: decimal(.2)}}}}
	s, err := New(path, "org_sites", r, c, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	return s, r, c, path
}
func integer(v int64) *int64     { return &v }
func decimal(v float64) *float64 { return &v }
func TestSnapshotSyncPersistenceFreshnessAndReadPurity(t *testing.T) {
	s, r, c, path := fixture(t)
	w := r.record.Website
	empty, err := s.Get(w.ID, w.Site, "algolia")
	if err != nil || empty.Status != "not_loaded" || c.calls != 0 || empty.Algolia != nil {
		t.Fatal(empty, err)
	}
	got, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia")
	if err != nil || got.Status != "connected" || *got.Algolia.Analytics.Searches != 10 {
		t.Fatal(got, err)
	}
	if got.Window.StartDate != "2026-10-16" || got.Window.EndDate != "2026-10-22" {
		t.Fatal(got.Window)
	}
	got.Algolia.Analytics.Searches = integer(999)
	read, _ := s.Get(w.ID, w.Site, "algolia")
	if *read.Algolia.Analytics.Searches != 10 || c.calls != 1 {
		t.Fatal("mutated state or fetched during GET")
	}
	restored, err := New(path, "org_sites", r, c, s.now)
	if err != nil {
		t.Fatal(err)
	}
	read, err = restored.Get(w.ID, w.Site, "algolia")
	if err != nil || read.Status != "connected" || c.calls != 1 {
		t.Fatal(read, err)
	}
	if _, err = s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); !errors.Is(err, ErrCooldown) {
		t.Fatal(err)
	}
	s.now = func() time.Time { return time.Date(2026, 10, 23, 18, 0, 0, 0, time.UTC) }
	read, err = s.Get(w.ID, w.Site, "algolia")
	if err != nil || read.Status != "not_loaded" || read.Algolia != nil {
		t.Fatal(read, err)
	}
	info, _ := os.Stat(path)
	if info.Mode().Perm() != 0600 {
		t.Fatal(info.Mode())
	}
}
func TestSnapshotScopeOwnershipRevisionAndProviderIsolation(t *testing.T) {
	s, r, c, _ := fixture(t)
	w := r.record.Website
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); err != nil {
		t.Fatal(err)
	}
	other, err := s.Get(w.ID, w.Site, "cookiebot")
	if err != nil || other.Status != "not_loaded" {
		t.Fatal(other, err)
	}
	r.record.Website.Revision++
	stale, err := s.Get(w.ID, w.Site, "algolia")
	if err != nil || stale.Status != "not_loaded" || stale.Algolia != nil {
		t.Fatal(stale, err)
	}
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); !errors.Is(err, websites.ErrConflict) {
		t.Fatal(err)
	}
	r.record.Website.Ownership.Status = "unverified"
	if got, err := s.Get(w.ID, w.Site, "algolia"); err != nil || got.Status != "not_loaded" {
		t.Fatal(err)
	}
	if c.calls != 1 {
		t.Fatal(c.calls)
	}
}
func TestSyncRejectsWrongOrMalformedReceiptAndConcurrentAuthorityChange(t *testing.T) {
	for _, change := range []func(*Report){
		func(r *Report) { r.WebsiteRevision++ }, func(r *Report) { r.Provider = "cookiebot" }, func(r *Report) { r.Window.EndDate = "2026-10-23" },
		func(r *Report) { r.ObservedAt = "2026-10-23T12:00:01Z" }, func(r *Report) { r.Algolia.Analytics.NoResultRate = decimal(.5) },
		func(r *Report) { r.Algolia.Analytics.Searches = integer(-1) }, func(r *Report) { r.Algolia.Analytics.ReportedDays = 8 },
		func(r *Report) { r.Algolia.Region = "eu" }, func(r *Report) { r.Algolia.Analytics.NoResultSearches = integer(11) },
	} {
		s, w, c, _ := fixture(t)
		site := w.record.Website
		change(&c.report)
		if _, err := s.Sync(t.Context(), site.ID, site.Site, site.Revision, "algolia"); !errors.Is(err, ErrUnavailable) {
			t.Fatal(c.report, err)
		}
		got, _ := s.Get(site.ID, site.Site, "algolia")
		if got.Status != "not_loaded" {
			t.Fatal(got)
		}
	}
	s, r, c, _ := fixture(t)
	w := r.record.Website
	c.after = func() { r.record.Website.Revision++ }
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); !errors.Is(err, websites.ErrConflict) {
		t.Fatal(err)
	}
}
func TestExplicitNotConfiguredDoesNotFabricateMetrics(t *testing.T) {
	s, r, c, _ := fixture(t)
	w := r.record.Website
	c.report.Status = "not_configured"
	c.report.Algolia = nil
	got, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia")
	if err != nil || got.Status != "not_configured" || got.Algolia != nil {
		t.Fatal(got, err)
	}
}
func TestExpiredCooldownEntriesDoNotBlockNewRegisteredSites(t *testing.T) {
	s, r, _, _ := fixture(t)
	w := r.record.Website
	for i := 0; i < maxEntries; i++ {
		s.attempts[fmt.Sprintf("old-%d", i)] = s.now().Add(-2 * time.Minute)
	}
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); err != nil {
		t.Fatal("expired cooldowns exhausted capacity", err)
	}
}

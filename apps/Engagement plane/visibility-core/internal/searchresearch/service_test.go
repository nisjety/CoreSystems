package searchresearch

import (
	"context"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/analyticssync"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"os"
	"path/filepath"
	"testing"
	"time"
)

const testSite = "https://triodelab.no/"
const testID = "web_11111111111111111111111111111111"

type registry struct{ revision int64 }

func (r *registry) OrganizationID() string { return "org-test" }
func (r *registry) Resolve(id, site string) (websites.Record, error) {
	if id != testID || site != testSite {
		return websites.Record{}, websites.ErrNotFound
	}
	return websites.Record{Website: websites.Website{ID: id, Site: site, Revision: r.revision}}, nil
}
func (r *registry) List() []websites.Website {
	return []websites.Website{{ID: testID, Site: testSite, Revision: r.revision}}
}

type authority struct {
	allow bool
	after func()
}

func (a *authority) Check(context.Context, websites.Record) analyticssync.Access {
	if a.after != nil {
		a.after()
	}
	return analyticssync.Access{SearchConsole: a.allow}
}

type collector struct {
	report pulse.SearchResearch
	calls  int
}

func (c *collector) CollectSearchResearch(context.Context, websites.Record) pulse.SearchResearch {
	c.calls++
	return c.report
}
func fixture(t *testing.T) (*Service, *registry, *authority, *collector, *time.Time) {
	t.Helper()
	dir := t.TempDir()
	_ = os.Chmod(dir, 0700)
	repo, err := Open(filepath.Join(dir, "research.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 10, 28, 12, 0, 0, 0, time.UTC)
	r := &registry{1}
	a := &authority{allow: true}
	c := &collector{report: pulse.SearchResearch{Site: testSite, Status: "connected", Period: pulse.Period{StartDate: "2026-07-28", EndDate: "2026-10-25"}, ObservedAt: stamp(now), Rows: []pulse.SearchObservation{}}}
	return New(repo, r, c, a, func() time.Time { return now }), r, a, c, &now
}
func TestObservedRelationshipsGroupsAndWeeklyWeights(t *testing.T) {
	s, r, _, c, _ := fixture(t)
	for _, day := range []string{"2026-10-21", "2026-10-22", "2026-10-23"} {
		c.report.Rows = append(c.report.Rows, pulse.SearchObservation{Date: day, Query: "webdesign", Page: testSite, Clicks: 1, Impressions: 10, Position: 2}, pulse.SearchObservation{Date: day, Query: "webdesign", Page: testSite + "web", Clicks: 0, Impressions: 5, Position: 5})
	}
	record, _ := r.Resolve(testID, testSite)
	s.collect(t.Context(), record, s.entry(record))
	v, err := s.Get(t.Context(), testID, testSite)
	if err != nil || len(v.Rows) != 6 || len(v.Relationships) != 1 || !v.Relationships[0].Candidate || v.Relationships[0].OverlapDays != 3 || v.Queries[0].Position != 3 {
		t.Fatal(v, err)
	}
	rule := Rule{ID: "web", Name: "Web services", Entity: "query", Contains: "WEB", MinImpressions: 20, MaxPosition: 4}
	v, err = s.Command(t.Context(), testID, testSite, 1, "save_rule", &rule)
	if err != nil || len(v.Groups) != 1 || len(v.Groups[0].Items) != 1 {
		t.Fatal(v, err)
	}
	calls := c.calls
	_, _ = s.Get(t.Context(), testID, testSite)
	if c.calls != calls {
		t.Fatal("GET started provider collection")
	}
	reopened, err := Open(s.repo.path, "org-test")
	if err != nil || len(reopened.read().Entries[0].Report.Rows) != 6 {
		t.Fatal(err)
	}
}
func TestRevocationRevisionAndFailedRefreshDoNotLeakOrReplaceSuccess(t *testing.T) {
	s, r, a, c, now := fixture(t)
	record, _ := r.Resolve(testID, testSite)
	s.collect(t.Context(), record, s.entry(record))
	c.report.Status = "unavailable"
	*now = now.Add(25 * time.Hour)
	s.collect(t.Context(), record, s.entry(record))
	v, err := s.Get(t.Context(), testID, testSite)
	if err != nil || v.Status != "connected" || v.Reason != "source_unavailable" || !v.Stale {
		t.Fatal(v, err)
	}
	a.allow = false
	v, _ = s.Get(t.Context(), testID, testSite)
	if v.Status != "unavailable" || len(v.Rows) > 0 || v.Reason != "authorization_required" {
		t.Fatal(v)
	}
	a.allow = true
	r.revision++
	v, _ = s.Get(t.Context(), testID, testSite)
	if v.Status != "unavailable" || len(v.Rules) > 0 {
		t.Fatal("old binding reused")
	}
	a.after = func() { r.revision++ }
	_, err = s.Get(t.Context(), testID, testSite)
	if !errors.Is(err, websites.ErrConflict) {
		t.Fatal(err)
	}
}
func TestUnsafePrivateAndMalformedRowsNeverPersist(t *testing.T) {
	s, _, _, c, _ := fixture(t)
	c.report.Rows = []pulse.SearchObservation{{Date: "2026-10-20", Query: "x", Page: testSite + "admin", Impressions: 1, Position: 1}}
	if validReport(testSite, &c.report) {
		t.Fatal("private URL accepted")
	}
	c.report.Rows[0].Page = testSite
	c.report.Rows[0].Clicks = 2
	if validReport(testSite, &c.report) {
		t.Fatal("impossible counts")
	}
	_ = s
}

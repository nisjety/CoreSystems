package trends

import (
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

type autoRegistryFixture struct{ record websites.Record }

func (r *autoRegistryFixture) List() []websites.Website { return []websites.Website{r.record.Website} }
func (r *autoRegistryFixture) Resolve(id, site string) (websites.Record, error) {
	if id != r.record.Website.ID || site != r.record.Website.Site {
		return websites.Record{}, websites.ErrNotFound
	}
	return r.record, nil
}
func (r *autoRegistryFixture) OrganizationID() string { return "org-test" }

type autoSourceFixture struct {
	search     pulse.SearchProjection
	now        *time.Time
	calls      int
	status     string
	observedAt string
	hook       func()
}

func (s *autoSourceFixture) TrendsSearch(context.Context, websites.Record) (pulse.SearchProjection, error) {
	return s.search, nil
}
func (s *autoSourceFixture) GetTrends(_ context.Context, _ string, _ string, _ int64, q Query) (Report, error) {
	s.calls++
	if s.hook != nil {
		s.hook()
	}
	status := s.status
	if status == "" {
		status = "available"
	}
	r := Report{Query: q, Source: "pytrends_dev", Scale: "relative_0_100_per_request", Status: status, ObservedAt: utc(*s.now), Rows: []Row{}}
	if s.observedAt != "" {
		r.ObservedAt = s.observedAt
		r.Cached = true
	}
	if status == "available" {
		values := make([]int, len(q.Terms))
		r.Rows = []Row{{Date: s.now.Add(-24 * time.Hour).Format(time.DateOnly), Values: values}}
	}
	if status == "rate_limited" {
		r.RetryAt = utc(s.now.Add(time.Hour))
	}
	return r, nil
}
func automaticFixture(t *testing.T) (*Automatic, *autoRegistryFixture, *autoSourceFixture, *time.Time, string) {
	t.Helper()
	now := time.Date(2026, 10, 24, 12, 0, 0, 0, time.UTC)
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "trends.json")
	registry := &autoRegistryFixture{websites.Record{Website: websites.Website{ID: "web_" + strings.Repeat("a", 32), Site: "https://triodelab.no/", Revision: 2}}}
	source := &autoSourceFixture{now: &now, search: pulse.SearchProjection{Site: registry.record.Website.Site, Source: "google_search_console", Status: "connected", UpdatedAt: utc(now), Period: pulse.Period{StartDate: "2026-10-01", EndDate: "2026-10-21"}, Queries: []pulse.SearchRow{{Key: "SEO", Impressions: 100}, {Key: "webdesign", Impressions: 200}}}}
	automatic, err := NewAutomatic(path, "org-test", registry, source, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	return automatic, registry, source, &now, path
}
func TestAutomaticTermsComeFromObservedImpressionsOnly(t *testing.T) {
	rows := []pulse.SearchRow{{Key: "low", Impressions: 1}, {Key: "highest", Impressions: 999}, {Key: "Highest", Impressions: 8}, {Key: "a", Impressions: 9}, {Key: "b", Impressions: 9, Clicks: 1}, {Key: "c", Impressions: 7}, {Key: "d", Impressions: 6}, {Key: "no evidence"}, {Key: strings.Repeat("ø", 41), Impressions: 1000}, {Key: "bad\nterm", Impressions: 1000}}
	got := SelectTerms(rows)
	if !reflect.DeepEqual(got, []string{"a", "b", "c", "d", "highest"}) {
		t.Fatal(got)
	}
}
func TestAutomaticPureReadDurableBackgroundAndRestart(t *testing.T) {
	a, r, s, now, path := automaticFixture(t)
	w := r.record.Website
	pending, err := a.Get(t.Context(), w.ID, w.Site)
	if err != nil || pending.Status != "pending" || s.calls != 0 {
		t.Fatal("read collected", pending, err)
	}
	a.cycle(t.Context())
	got, err := a.Get(t.Context(), w.ID, w.Site)
	if err != nil || got.Status != "complete" || got.Report == nil || got.Report.Geo != "" || len(got.Report.Terms) != 2 || s.calls != 1 {
		t.Fatal(got, err, s.calls)
	}
	if got.Seed.Period.StartDate != "2026-10-01" || got.Report.Rows[0].Values[0] != 0 {
		t.Fatal("lost evidence or measured zero")
	}
	again, err := NewAutomatic(path, "org-test", r, s, func() time.Time { return *now })
	if err != nil {
		t.Fatal(err)
	}
	again.cycle(t.Context())
	if s.calls != 1 {
		t.Fatal("restart repeated fresh provider collection")
	}
	info, _ := os.Stat(path)
	if info.Mode().Perm() != 0600 {
		t.Fatal("not private")
	}
}
func TestAutomaticMissingOrRevokedKeywordEvidenceHidesSavedData(t *testing.T) {
	for _, mode := range []string{"no_source", "no_keywords", "expired"} {
		t.Run(mode, func(t *testing.T) {
			a, r, s, now, _ := automaticFixture(t)
			w := r.record.Website
			a.cycle(t.Context())
			switch mode {
			case "no_source":
				s.search.Status = "unavailable"
			case "no_keywords":
				s.search.Queries = nil
			case "expired":
				*now = now.Add(7 * time.Hour)
			}
			got, err := a.Get(t.Context(), w.ID, w.Site)
			if err != nil || got.Report != nil || got.Seed != nil || s.calls != 1 {
				t.Fatal("private seed leaked or read fetched", got, err)
			}
			a.cycle(t.Context())
			if a.repo.get(w.ID).Report != nil {
				t.Fatal("revoked derived data retained")
			}
		})
	}
}
func TestAutomaticChangedKeywordSetInvalidatesOldReport(t *testing.T) {
	a, r, s, _, _ := automaticFixture(t)
	w := r.record.Website
	a.cycle(t.Context())
	s.search.Queries = []pulse.SearchRow{{Key: "new actual query", Impressions: 300}}
	got, _ := a.Get(t.Context(), w.ID, w.Site)
	if got.Status != "pending" || got.Report != nil {
		t.Fatal("old query set shown")
	}
	a.cycle(t.Context())
	got, _ = a.Get(t.Context(), w.ID, w.Site)
	if s.calls != 2 || got.Report == nil || got.Report.Terms[0] != "new actual query" {
		t.Fatal("changed keywords did not refresh", got)
	}
}
func TestAutomaticRefusalBackoffSurvivesRestart(t *testing.T) {
	a, r, s, now, path := automaticFixture(t)
	s.status = "rate_limited"
	w := r.record.Website
	a.cycle(t.Context())
	got, _ := a.Get(t.Context(), w.ID, w.Site)
	if got.Status != "failed" || got.Report == nil || got.Report.Status != "rate_limited" {
		t.Fatal(got)
	}
	*now = now.Add(10 * time.Minute)
	again, err := NewAutomatic(path, "org-test", r, s, func() time.Time { return *now })
	if err != nil {
		t.Fatal(err)
	}
	again.cycle(t.Context())
	if s.calls != 1 {
		t.Fatal("quota refusal bypassed")
	}
	*now = now.Add(time.Hour)
	s.status = "available"
	again.cycle(t.Context())
	if s.calls != 2 {
		t.Fatal("retry did not resume")
	}
}
func TestAutomaticRechecksContextAndAuthorityAfterCollection(t *testing.T) {
	for _, mode := range []string{"revision", "revoke"} {
		t.Run(mode, func(t *testing.T) {
			a, r, s, _, _ := automaticFixture(t)
			w := r.record.Website
			s.hook = func() {
				if mode == "revision" {
					r.record.Website.Revision++
				} else {
					s.search.Status = "unavailable"
				}
			}
			a.cycle(t.Context())
			got, err := a.Get(t.Context(), w.ID, w.Site)
			if err != nil || got.Report != nil {
				t.Fatal("changed context leaked", got, err)
			}
		})
	}
}
func TestAutomaticRepositoryRejectsAnotherOrganization(t *testing.T) {
	a, _, _, _, path := automaticFixture(t)
	a.cycle(t.Context())
	if _, err := openAutomatic(path, "other-org"); err == nil {
		t.Fatal("cross-organization storage accepted")
	}
}

func TestAutomaticExpiryUsesOriginalProviderObservation(t *testing.T) {
	a, r, s, now, _ := automaticFixture(t)
	w := r.record.Website
	s.observedAt = utc(now.Add(-23 * time.Hour))
	a.cycle(t.Context())
	got, err := a.Get(t.Context(), w.ID, w.Site)
	if err != nil || got.Report == nil || !timeValue(got.ExpiresAt).Equal(now.Add(time.Hour)) {
		t.Fatal("cache read extended freshness", got, err)
	}
	*now = now.Add(2 * time.Hour)
	s.search.UpdatedAt = utc(*now)
	got, err = a.Get(t.Context(), w.ID, w.Site)
	if err != nil || got.Report != nil || got.Status != "pending" {
		t.Fatal("expired report shown", got, err)
	}
	s.observedAt = ""
	a.cycle(t.Context())
	if s.calls != 2 {
		t.Fatal("expired report not refreshed")
	}
}

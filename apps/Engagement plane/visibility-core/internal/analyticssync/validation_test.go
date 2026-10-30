package analyticssync

import (
	"context"
	"math"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

func TestSourceAgeNotExtendedByCollectionReceipt(t *testing.T) {
	s, r, a, _, _ := fixture(t)
	w := r.record.Website
	_, _ = s.Queue(w.ID, w.Site, 1, "Q4")
	e, _ := s.claim()
	bundle := completeBundle(t)
	bundle.Clarity.UpdatedAt = stamp(s.now().Add(-7 * time.Hour))
	bundle.Clarity.Window.ObservedAt = bundle.Clarity.UpdatedAt
	e = completedEntry(e, bundle, s.now())
	if err := s.repo.update(func(v diskState) (diskState, error) { v.Entries[0] = e; return v, nil }); err != nil {
		t.Fatal(err)
	}
	a.mu.Lock()
	a.access = Access{SearchConsole: true, GA4: true, Clarity: true, ClarityProjectID: bundle.Clarity.ProjectID, ClarityVersion: bundle.Clarity.BindingVersion}
	a.mu.Unlock()
	got, err := s.Read(context.Background(), w.ID, w.Site, "Q4")
	if err != nil || got.Clarity.Metrics != nil || got.Clarity.Reason != "snapshot_expired" || got.GA4.Totals == nil || got.Pulse.Totals == nil {
		t.Fatal("oldproviderobservationrenewed", got, err)
	}
}

func completeBundle(t *testing.T) Bundle {
	t.Helper()
	s, r, _, runner, _ := fixture(t)
	b, err := runner.Collect(t.Context(), r.record, "Q4")
	if err != nil {
		t.Fatal(err)
	}
	row := pulse.SearchRow{Key: "triodelab", Clicks: 2, Impressions: 200, CTR: 0.01, Position: 3}
	b.Search.Queries = []pulse.SearchRow{row}
	b.Search.Pages = []pulse.SearchRow{{Key: "https://triodelab.no/", Clicks: 2, Impressions: 200, CTR: 0.01, Position: 3}}
	b.Search.Opportunities = []pulse.Opportunity{{SearchRow: row, Type: "snippet_review"}}
	b.GA4 = pulse.GA4Projection{Site: b.Pulse.Site, Source: "google_analytics", Status: "connected", Period: b.Pulse.Period, Totals: &pulse.GA4Metrics{Users: 2, Sessions: 3, EngagedSessions: 2, PageViews: 5, KeyEvents: 1.5, EngagementSeconds: 12}, Days: []pulse.GA4Day{{Date: b.Pulse.Period.StartDate, GA4Metrics: pulse.GA4Metrics{Users: 1, Sessions: 1}}}, TimeZone: "Europe/Oslo", UpdatedAt: stamp(s.now())}
	n := int64(2)
	p := float64(1.2)
	b.Clarity = pulse.ClarityProjection{Site: b.Pulse.Site, Source: "microsoft_clarity", Status: "connected", Scope: "clarity_project", ProjectID: "project1", BindingVersion: strings.Repeat("a", 64), Window: pulse.ClarityWindow{NumOfDays: 3, TimeZone: "UTC", ObservedAt: stamp(s.now())}, Metrics: &pulse.ClarityMetrics{Sessions: &n, Users: &n, BotSessions: &n, PagesPerSession: &p}, UpdatedAt: stamp(s.now())}
	return b
}
func TestBundleValidationAllProjections(t *testing.T) {
	now := time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC)
	valid := completeBundle(t)
	if !validBundle(valid, "https://triodelab.no/", "Q4", now) {
		t.Fatal("fixture invalid")
	}
	cases := map[string]func(*Bundle){
		"nanopportunity": func(b *Bundle) { b.Search.Opportunities[0].CTR = math.NaN() }, "infopportunity": func(b *Bundle) { b.Search.Opportunities[0].Position = math.Inf(1) }, "opportunitytype": func(b *Bundle) { b.Search.Opportunities[0].Type = "invented" }, "opportunityevidence": func(b *Bundle) { b.Search.Opportunities[0].Clicks = 1 }, "badcalendar": func(b *Bundle) { b.Pulse.Days[0].Date = "2026-10-00" }, "duplicateday": func(b *Bundle) { b.Pulse.Days = append(b.Pulse.Days, b.Pulse.Days[0]) }, "future": func(b *Bundle) { b.Pulse.UpdatedAt = stamp(now.Add(time.Hour)) }, "clarityfuture": func(b *Bundle) { b.Clarity.Window.ObservedAt = stamp(now.Add(time.Hour)) }, "ga4zone": func(b *Bundle) { b.GA4.TimeZone = "Local" }, "ga4foreign": func(b *Bundle) { b.GA4.ReportingSite = "https://other.no/" }, "clarityversion": func(b *Bundle) { b.Clarity.BindingVersion = "" }, "claritynull": func(b *Bundle) { b.Clarity.Metrics = &pulse.ClarityMetrics{} }, "clarityproject": func(b *Bundle) { b.Clarity.ProjectID = "../other" }, "negative": func(b *Bundle) { b.GA4.Totals.Users = -1 }, "nanmetric": func(b *Bundle) { b.GA4.Totals.KeyEvents = math.NaN() }, "scope": func(b *Bundle) { b.Search.PropertyScope = "all" }, "ctr": func(b *Bundle) { b.Search.Queries[0].CTR = .5 }, "foreignpage": func(b *Bundle) { b.Search.Pages[0].Key = "https://foreign.no/" }, "badkey": func(b *Bundle) { b.Search.Queries[0].Key = "\x00" }, "duplicaterow": func(b *Bundle) { b.Search.Queries = append(b.Search.Queries, b.Search.Queries[0]) }, "wrongperiod": func(b *Bundle) { b.GA4.Period.StartDate = "2026-09-01" }, "unavailmetrics": func(b *Bundle) { b.Clarity.Status = "unavailable"; b.Clarity.Reason = "no_data" }, "missingtotals": func(b *Bundle) { b.Pulse.Totals = nil }, "negativeclarity": func(b *Bundle) { n := int64(-1); b.Clarity.Metrics.Users = &n }, "nanclarity": func(b *Bundle) { n := math.NaN(); b.Clarity.Metrics.PagesPerSession = &n }, "badga4day": func(b *Bundle) { b.GA4.Days[0].Date = "2026-11-31" }, "nofiniteposition": func(b *Bundle) { b.Search.Pages[0].Position = -1 }, "limitsignal": func(b *Bundle) { b.Search.PagesAtLimit = true }, "connectedreason": func(b *Bundle) { b.Pulse.Reason = "no_data" }, "source": func(b *Bundle) { b.Pulse.Source = "raw" }, "daycount": func(b *Bundle) { b.GA4.Days = make([]pulse.GA4Day, 100) },
	}
	for name, change := range cases {
		t.Run(name, func(t *testing.T) {
			b := clone(valid)
			change(&b)
			if validBundle(b, "https://triodelab.no/", "Q4", now) {
				t.Fatal("invalid accepted")
			}
		})
	}
	empty := valid
	empty.GA4.Totals = nil
	empty.GA4.Days = []pulse.GA4Day{}
	if !validBundle(empty, "https://triodelab.no/", "Q4", now) {
		t.Fatal("genuine GA4 missing totals rejected")
	}
}
func TestStateValidationAndCompleteOutcome(t *testing.T) {
	s, r, _, _, _ := fixture(t)
	w := r.record.Website
	_, _ = s.Queue(w.ID, w.Site, 1, "Q4")
	e, _ := s.claim()
	e = completedEntry(e, completeBundle(t), s.now())
	if e.Status != "complete" {
		t.Fatal(e)
	}
	state := diskState{Version: 1, OrganizationID: "org_test", Entries: []entry{e}}
	if err := validateState(state); err != nil {
		t.Fatal(err)
	}
	cases := []func(*diskState){func(v *diskState) { v.Version = 9 }, func(v *diskState) { v.Entries[0].Steps = nil }, func(v *diskState) { v.Entries[0].Fingerprint = "invalid" }, func(v *diskState) { v.Entries[0].JobID = "bad" }, func(v *diskState) { v.Entries[0].CompletedAt = "bad" }, func(v *diskState) { v.Entries[0].CompletedAt = stamp(s.now().Add(-time.Hour)) }, func(v *diskState) { v.Entries[0].Steps[0].Status = "running" }, func(v *diskState) { v.Entries[0].Steps[0].ObservedAt = "bad" }, func(v *diskState) { v.Entries[0].ExpiresAt = stamp(s.now().Add(7 * time.Hour)) }, func(v *diskState) { v.Entries = append(v.Entries, v.Entries[0]) }, func(v *diskState) { v.Entries[0].Status = "injected" }, func(v *diskState) { v.Entries[0].QueuedAt = "" }, func(v *diskState) { v.Entries[0].Steps[1].Reason = "unknown" }, func(v *diskState) { v.Entries[0].Status = "failed" }}
	for i, change := range cases {
		v := clone(state)
		change(&v)
		if validateState(v) == nil {
			t.Fatal("badstate accepted", i)
		}
	}
}

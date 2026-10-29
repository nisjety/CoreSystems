package analyticssync

import (
	"context"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

func TestSuccessHistorySurvivesFailedRefreshAndRestartWithoutMetrics(t *testing.T) {
	s, r, a, runner, path := fixture(t)
	w := r.record.Website
	if _, err := s.Queue(w.ID, w.Site, 1, "Q4"); err != nil {
		t.Fatal(err)
	}
	e, _ := s.claim()
	b := completeBundle(t)
	b.Clarity.UpdatedAt = stamp(s.now().Add(-time.Hour))
	b.Clarity.Window.ObservedAt = b.Clarity.UpdatedAt
	e = completedEntry(e, b, s.now())
	if e.Steps[2].LastSuccessAt != b.Clarity.UpdatedAt {
		t.Fatal("cached observation renewed")
	}
	if err := s.repo.update(func(v diskState) (diskState, error) { v.Entries[0] = e; return v, nil }); err != nil {
		t.Fatal(err)
	}
	now := s.now().Add(2 * time.Hour)
	s.now = func() time.Time { return now }
	queued, err := s.Queue(w.ID, w.Site, 1, "Q4")
	if err != nil {
		t.Fatal(err)
	}
	if queued.Steps[0].LastSuccessAt != b.Pulse.UpdatedAt {
		t.Fatal("queue lost success history")
	}
	retry, _ := s.claim()
	retry = failedEntry(retry, now, "timeout")
	if err = s.repo.update(func(v diskState) (diskState, error) { v.Entries[0] = retry; return v, nil }); err != nil {
		t.Fatal(err)
	}
	repo, err := Open(path, "org_test")
	if err != nil {
		t.Fatal(err)
	}
	s = New(repo, r, runner, a, s.now)
	got, err := s.Get(w.ID, w.Site, "Q4")
	if err != nil || got.Status != "failed" || got.Steps[0].LastSuccessAt != b.Pulse.UpdatedAt {
		t.Fatal(got, err)
	}
	data, err := s.Read(context.Background(), w.ID, w.Site, "Q4")
	if err != nil || data.Pulse.Totals != nil || data.GA4.Totals != nil || data.Clarity.Metrics != nil {
		t.Fatal("failure served old metrics", err)
	}
	for _, invalid := range []string{"bad", stamp(now.Add(time.Hour))} {
		v := repo.read()
		v.Entries[0].Steps[0].LastSuccessAt = invalid
		if validateState(v) == nil {
			t.Fatal("invalid history accepted")
		}
	}
	r.mu.Lock()
	r.record.GSCConnectionID = "replacement"
	r.mu.Unlock()
	got, err = s.Get(w.ID, w.Site, "Q4")
	if err != nil || got.Steps[0].LastSuccessAt != "" {
		t.Fatal("new binding inherited history", err)
	}
}

func TestFullYearSnapshotPersistsMoreThanQuarterOfDays(t *testing.T) {
	s, r, _, runner, path := fixture(t)
	w := r.record.Website
	if _, err := s.Queue(w.ID, w.Site, 1, "FY"); err != nil {
		t.Fatal(err)
	}
	e, ok := s.claim()
	if !ok {
		t.Fatal("full year not claimable")
	}
	b, err := runner.Collect(t.Context(), r.record, "FY")
	if err != nil {
		t.Fatal(err)
	}
	b.Pulse.Days = nil
	start, _ := time.Parse("2006-01-02", b.Pulse.Period.StartDate)
	for i := 0; i < 150; i++ {
		b.Pulse.Days = append(b.Pulse.Days, pulse.Day{Date: start.AddDate(0, 0, i).Format("2006-01-02")})
	}
	if !validBundle(b, w.Site, "FY", s.now()) {
		t.Fatal("full-year days rejected")
	}
	e = completedEntry(e, b, s.now())
	if err = s.repo.update(func(v diskState) (diskState, error) { v.Entries[0] = e; return v, nil }); err != nil {
		t.Fatal(err)
	}
	if _, err = Open(path, "org_test"); err != nil {
		t.Fatal("full-year lost on restart", err)
	}
	if validBundle(b, w.Site, "Q4", s.now()) {
		t.Fatal("quarter accepted full year")
	}
}

func TestNextCollectionPrecedesCachedSourceExpiry(t *testing.T) {
	s, r, _, _, _ := fixture(t)
	w := r.record.Website
	_, _ = s.Queue(w.ID, w.Site, 1, "Q4")
	entry, _ := s.claim()
	bundle := completeBundle(t)
	bundle.Clarity.UpdatedAt = stamp(s.now().Add(-4 * time.Hour))
	bundle.Clarity.Window.ObservedAt = bundle.Clarity.UpdatedAt
	result := completedEntry(entry, bundle, s.now())
	if result.NextSyncAt != stamp(s.now().Add(2*time.Hour)) {
		t.Fatal("cached source allowed to expire before refresh", result.NextSyncAt)
	}
	bundle.Clarity.UpdatedAt = stamp(s.now().Add(-7 * time.Hour))
	bundle.Clarity.Window.ObservedAt = bundle.Clarity.UpdatedAt
	result = completedEntry(entry, bundle, s.now())
	if result.NextSyncAt != stamp(s.now().Add(retryDelay)) {
		t.Fatal("expired cache caused immediate retry loop")
	}
}

func TestRestartRepairsEarlierCachedSourceExpiryWithoutCollecting(t *testing.T) {
	s, r, _, runner, _ := fixture(t)
	w := r.record.Website
	_, _ = s.Queue(w.ID, w.Site, 1, "Q4")
	e, _ := s.claim()
	bundle := completeBundle(t)
	bundle.Clarity.UpdatedAt = stamp(s.now().Add(-4 * time.Hour))
	bundle.Clarity.Window.ObservedAt = bundle.Clarity.UpdatedAt
	e = completedEntry(e, bundle, s.now())
	e.NextSyncAt = stamp(s.now().Add(metricsTTL))
	if err := s.repo.update(func(v diskState) (diskState, error) { v.Entries[0] = e; return v, nil }); err != nil {
		t.Fatal(err)
	}
	if err := s.recover(); err != nil {
		t.Fatal(err)
	}
	got, err := s.Get(w.ID, w.Site, "Q4")
	if err != nil || got.NextSyncAt != stamp(s.now().Add(2*time.Hour)) || runner.calls != 0 {
		t.Fatal("restart failed to repair schedule without provider work", got, err)
	}
}

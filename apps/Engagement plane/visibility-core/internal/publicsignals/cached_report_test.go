package publicsignals

import (
	"context"
	"errors"
	"testing"
	"time"
)

func TestRefreshKeepsPriorObservationThroughFailureAndRestart(t *testing.T) {
	s, registry, runner, path := fixture(t)
	first := complete(t, s, registry)
	now := runner.now.Add(25 * time.Hour)
	s.now = func() time.Time { return now }
	expired, err := s.Get(first.WebsiteID, first.Site)
	if err != nil || expired.Report != nil || expired.CachedReport == nil || runner.calls != 1 {
		t.Fatal("read must return labelled cache without collecting", err)
	}
	running, ok := s.claim()
	if !ok || running.CachedReport == nil || running.Report != nil {
		t.Fatal("scheduled refresh discarded previous evidence")
	}
	runner.err = errors.New("upstream failed")
	s.execute(context.Background(), running)
	failed, err := s.Get(first.WebsiteID, first.Site)
	if err != nil || failed.Status != "failed" || failed.CachedReport == nil || failed.CachedReport.ObservedAt != first.Report.ObservedAt {
		t.Fatal("failure lost or restamped original observation")
	}
	repo, err := Open(path, registry.OrganizationID())
	if err != nil {
		t.Fatal(err)
	}
	s = New(repo, registry, runner, func() time.Time { return now })
	read, err := s.Get(first.WebsiteID, first.Site)
	if err != nil || read.CachedReport == nil || runner.calls != 2 {
		t.Fatal("cache not restart-safe or read collected data")
	}
	_, err = s.SetEnabled(first.WebsiteID, first.Site, 1, false)
	if err != nil {
		t.Fatal(err)
	}
	now = now.Add(24 * time.Hour)
	s.claim()
	if s.repo.read().Entries[0].CachedReport != nil {
		t.Fatal("paused cache exceeded original 48-hour retention")
	}
	read, err = s.Get(first.WebsiteID, first.Site)
	if err != nil || read.Report != nil || read.CachedReport != nil {
		t.Fatal("expired evidence leaked")
	}
}

func TestManualRefreshCacheDoesNotCrossWebsiteRevision(t *testing.T) {
	s, r, runner, _ := fixture(t)
	first := complete(t, s, r)
	now := runner.now.Add(2 * time.Minute)
	s.now = func() time.Time { return now }
	queued, err := s.Queue(first.WebsiteID, first.Site, 1)
	if err != nil || queued.CachedReport == nil || queued.Report != nil {
		t.Fatal("manual refresh must keep separate cache", err)
	}
	r.record.Website.Revision++
	read, err := s.Get(first.WebsiteID, first.Site)
	if err != nil || read.Status != "idle" || read.CachedReport != nil {
		t.Fatal("previous revision cache leaked")
	}
}

func TestPartialProviderFailureRetriesBeforeDailyRefresh(t *testing.T) {
	s, r, runner, _ := fixture(t)
	report := testReport(r.record.Website.Site, runner.now)
	report.CommonCrawl.Reason = "timeout"
	s.runner = timeoutReportRunner{report}
	got := complete(t, s, r)
	if got.Status != "complete" || got.Report == nil || parse(got.NextSyncAt).Sub(parse(got.CompletedAt)) != failureDelay {
		t.Fatal("provider failure was deferred for an entire day")
	}
}

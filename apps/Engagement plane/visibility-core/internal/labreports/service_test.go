package labreports

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

const site = "https://triodelab.no/"
const id = "web_11111111111111111111111111111111"

type registry struct {
	website   websites.Website
	org       string
	afterRead func()
}

func (r *registry) OrganizationID() string   { return r.org }
func (r *registry) List() []websites.Website { return []websites.Website{r.website} }
func (r *registry) Resolve(_ string, url string) (websites.Record, error) {
	if r.website.Site != url {
		return websites.Record{}, websites.ErrNotFound
	}
	w := r.website
	if r.afterRead != nil {
		r.afterRead()
	}
	return websites.Record{Website: w}, nil
}

type provider struct {
	starts, gets int
	result       pulse.PageSpeedProjection
	afterStart   func()
}

func (p *provider) StartPageSpeed(context.Context, string, string) pulse.PageSpeedProjection {
	p.starts++
	if p.afterStart != nil {
		p.afterStart()
	}
	return p.result
}
func (p *provider) GetPageSpeed(context.Context, string, string) pulse.PageSpeedProjection {
	p.gets++
	return p.result
}
func report(now time.Time, device string) pulse.PageSpeedProjection {
	score := 91.0
	return pulse.PageSpeedProjection{Site: site, Source: "google_pagespeed", Strategy: device, Status: "complete", RequestedAt: timestamp(now), CompletedAt: timestamp(now), Report: &pulse.PageSpeedReport{FetchTime: timestamp(now), LighthouseVersion: "13.0.0", Performance: &score}}
}
func fixture(t *testing.T) (*Service, *Repository, *registry, *provider, *time.Time) {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := Open(filepath.Join(dir, "labs.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 10, 28, 12, 0, 0, 0, time.UTC)
	r := &registry{org: "org-test", website: websites.Website{ID: id, Site: site, Revision: 1}}
	p := &provider{result: report(now, "mobile")}
	return New(repo, r, p, func() time.Time { return now }), repo, r, p, &now
}
func complete(t *testing.T, s *Service) {
	t.Helper()
	job, ok := s.claim()
	if !ok {
		t.Fatal("no durable claim")
	}
	s.collect(t.Context(), job)
}
func TestDurableReportsServeWithoutProviderAndRefreshAtExactlyOneDay(t *testing.T) {
	s, repo, r, p, now := fixture(t)
	if got := s.Queue(site, "mobile"); got.Status != "running" {
		t.Fatal(got)
	}
	complete(t, s)
	opened, err := Open(repo.path, r.org)
	if err != nil {
		t.Fatal(err)
	}
	restarted := New(opened, r, nil, func() time.Time { return *now })
	got := restarted.Get(t.Context(), site, "mobile")
	if got.Report == nil || *got.Report.Performance != 91 || got.Cache.Stale || got.Cache.RefreshStatus != "complete" || p.starts != 1 || p.gets != 0 {
		t.Fatal(got)
	}
	*now = now.Add(24*time.Hour - time.Nanosecond)
	s.ensure()
	if got := s.Get(t.Context(), site, "mobile"); got.Cache.RefreshStatus != "complete" || got.Cache.Stale {
		t.Fatal("refreshed too early", got)
	}
	*now = now.Add(time.Nanosecond)
	s.ensure()
	got = s.Get(t.Context(), site, "mobile")
	if got.Status != "complete" || got.Report == nil || !got.Cache.Stale || got.Cache.RefreshStatus != "queued" {
		t.Fatal("last good result must remain visible", got)
	}
	p.result = pulse.PageSpeedProjection{Status: "failed", Reason: "quota_limited"}
	complete(t, s)
	got = s.Get(t.Context(), site, "mobile")
	if got.Status != "complete" || got.Report == nil || !got.Cache.Stale || got.Cache.RefreshStatus != "failed" || got.Cache.RefreshReason != "quota_limited" {
		t.Fatal("failure discarded last success", got)
	}
	s.ensure()
	if s.Get(t.Context(), site, "mobile").Cache.RefreshStatus != "failed" {
		t.Fatal("ignored retry backoff")
	}
	*now = now.Add(time.Hour)
	s.ensure()
	p.result = report(*now, "mobile")
	complete(t, s)
	got = s.Get(t.Context(), site, "mobile")
	if got.Cache.Stale || got.Cache.RefreshStatus != "complete" || got.Cache.LastSuccessAt != timestamp(*now) {
		t.Fatal(got)
	}
}
func TestWebsiteRevisionDeviceAndOrganizationIsolation(t *testing.T) {
	s, repo, r, _, _ := fixture(t)
	s.Queue(site, "mobile")
	complete(t, s)
	if s.Get(t.Context(), site, "desktop").Report != nil || s.Get(t.Context(), "https://other.no/", "mobile").Report != nil {
		t.Fatal("cross-context report")
	}
	r.website.Revision++
	if s.Get(t.Context(), site, "mobile").Report != nil {
		t.Fatal("old revision visible")
	}
	s.Queue(site, "mobile")
	if s.Get(t.Context(), site, "mobile").Report != nil {
		t.Fatal("old success retained for new revision")
	}
	if _, err := Open(repo.path, "different-org"); err == nil {
		t.Fatal("cross-org repository accepted")
	}
	r.org = "different-org"
	if s.Get(t.Context(), site, "mobile").Report != nil {
		t.Fatal("wrong authority visible")
	}
}
func TestReadsAndIdleWorkersDoNotWriteAndFailedClaimCannotCallProvider(t *testing.T) {
	s, repo, _, p, _ := fixture(t)
	writes := 0
	persist := repo.persistFn
	repo.persistFn = func(v diskState) error { writes++; return persist(v) }
	for range 3 {
		s.Get(t.Context(), site, "mobile")
		if _, ok := s.claim(); ok {
			t.Fatal("idle claim")
		}
	}
	if writes != 0 {
		t.Fatal("idle filesystem churn", writes)
	}
	s.Queue(site, "mobile")
	repo.persistFn = func(diskState) error { return errors.New("disk full") }
	if _, ok := s.claim(); ok {
		t.Fatal("unpersisted claim")
	}
	if p.starts != 0 {
		t.Fatal("provider used without durable claim")
	}
	got := s.Queue(site, "desktop")
	if got.Status != "failed" || got.Reason != "source_unavailable" {
		t.Fatal("failed write hidden", got)
	}
}
func TestNeverReplaceWithOlderFutureOrWrongOriginReport(t *testing.T) {
	for _, kind := range []string{"older", "future", "wrong-site", "wrong-device"} {
		t.Run(kind, func(t *testing.T) {
			s, _, _, p, now := fixture(t)
			s.Queue(site, "mobile")
			complete(t, s)
			first := s.Get(t.Context(), site, "mobile").Report.FetchTime
			*now = now.Add(25 * time.Hour)
			s.Queue(site, "mobile")
			p.result = report(*now, "mobile")
			switch kind {
			case "older":
				p.result.Report.FetchTime = timestamp(now.Add(-26 * time.Hour))
			case "future":
				p.result.Report.FetchTime = timestamp(now.Add(time.Hour))
			case "wrong-site":
				p.result.Site = "https://other.no/"
			case "wrong-device":
				p.result.Strategy = "desktop"
			}
			complete(t, s)
			got := s.Get(t.Context(), site, "mobile")
			if got.Report.FetchTime != first || got.Cache.RefreshStatus != "failed" {
				t.Fatal(got)
			}
		})
	}
}
func TestContextChangeDuringReadOrCollectionNeverPublishesOldResult(t *testing.T) {
	s, _, r, p, _ := fixture(t)
	s.Queue(site, "mobile")
	p.afterStart = func() { r.website.Revision++ }
	complete(t, s)
	if s.Get(t.Context(), site, "mobile").Report != nil {
		t.Fatal("old result published")
	}
	p.afterStart = nil
	s.Queue(site, "mobile")
	complete(t, s)
	calls := 0
	r.afterRead = func() {
		calls++
		if calls == 1 {
			r.website.Revision++
		}
	}
	if s.Get(t.Context(), site, "mobile").Report != nil {
		t.Fatal("mid-read context change returned old result")
	}
}
func TestFinishFailureRetainsRunningReceiptUntilDurablySaved(t *testing.T) {
	s, repo, _, p, _ := fixture(t)
	s.Queue(site, "mobile")
	job, ok := s.claim()
	if !ok {
		t.Fatal("claim")
	}
	persist := repo.persistFn
	repo.persistFn = func(diskState) error { return errors.New("disk full") }
	if s.finish(job, p.result) == nil {
		t.Fatal("storage failure hidden")
	}
	if repo.read().Reports[0].Status != "running" {
		t.Fatal("unpersisted result published")
	}
	repo.persistFn = persist
	if err := s.finish(job, p.result); err != nil {
		t.Fatal(err)
	}
	if s.Get(t.Context(), site, "mobile").Report == nil {
		t.Fatal("result not saved")
	}
}

func TestRestartRecoversInterruptedReceiptsWithoutOpeningThePage(t *testing.T) {
	s, repo, r, _, now := fixture(t)
	s.Queue(site, "mobile")
	if _, ok := s.claim(); !ok {
		t.Fatal("claim")
	}
	reopened, err := Open(repo.path, r.org)
	if err != nil {
		t.Fatal(err)
	}
	restarted := New(reopened, r, nil, func() time.Time { return *now })
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	done := make(chan struct{})
	go func() { defer close(done); restarted.Run(ctx) }()
	deadline := time.After(2 * time.Second)
	for {
		rows := reopened.read().Reports
		if len(rows) == 2 && rows[0].Status == "failed" && rows[1].Status == "failed" {
			break
		}
		select {
		case <-deadline:
			t.Fatal("scheduler did not recover and prepare both devices")
		case <-time.After(5 * time.Millisecond):
		}
	}
	cancel()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("workers ignored shutdown")
	}
}

package publicsignals

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type registryFake struct{ record websites.Record }

func (r *registryFake) Resolve(id, site string) (websites.Record, error) {
	if id != r.record.Website.ID || site != r.record.Website.Site {
		return websites.Record{}, websites.ErrNotFound
	}
	return r.record, nil
}
func (*registryFake) OrganizationID() string { return "org_public" }

type runnerFake struct {
	calls int
	now   time.Time
	err   error
}

func (r *runnerFake) Collect(_ context.Context, w websites.Record) (Report, error) {
	r.calls++
	return testReport(w.Website.Site, r.now), r.err
}
func testReport(site string, now time.Time) Report {
	return Report{Site: site, Source: "public_web_signals", ObservedAt: stamp(now), CrUX: []CrUX{{FormFactor: "PHONE", Status: "unavailable", Reason: "not_configured"}, {FormFactor: "DESKTOP", Status: "unavailable", Reason: "no_data"}}, CommonCrawl: CommonCrawl{Status: "unavailable", Reason: "no_data", Captures: []Capture{}, Limit: 20}}
}
func fixture(t *testing.T) (*Service, *registryFake, *runnerFake, string) {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "public.json")
	repo, err := Open(path, "org_public")
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC)
	registry := &registryFake{websites.Record{Website: websites.Website{ID: "web_00000000000000000000000000000001", Site: "https://triodelab.no/", Revision: 1}}}
	runner := &runnerFake{now: now}
	return New(repo, registry, runner, func() time.Time { return now }), registry, runner, path
}
func complete(t *testing.T, s *Service, r *registryFake) State {
	t.Helper()
	w := r.record.Website
	if _, err := s.Queue(w.ID, w.Site, 1); err != nil {
		t.Fatal(err)
	}
	e, ok := s.claim()
	if !ok {
		t.Fatal("claimfailed")
	}
	s.execute(context.Background(), e)
	v, err := s.Get(w.ID, w.Site)
	if err != nil {
		t.Fatal(err)
	}
	return v
}
func TestGetIsPureAndQueueDurable(t *testing.T) {
	s, r, runner, path := fixture(t)
	w := r.record.Website
	v, err := s.Get(w.ID, w.Site)
	if err != nil || v.Status != "idle" || v.Report != nil || runner.calls != 0 {
		t.Fatal(v, err)
	}
	v = complete(t, s, r)
	if v.Status != "complete" || v.Report == nil || runner.calls != 1 {
		t.Fatal(v)
	}
	repo, err := Open(path, "org_public")
	if err != nil {
		t.Fatal(err)
	}
	restored := New(repo, r, runner, s.now)
	after, err := restored.Get(w.ID, w.Site)
	if err != nil || after.JobID != v.JobID || runner.calls != 1 {
		t.Fatal(after, err)
	}
	if _, err = s.Queue(w.ID, w.Site, 1); !errors.Is(err, ErrCooldown) {
		t.Fatal(err)
	}
}
func TestExpiryRevisionPauseAndNoData(t *testing.T) {
	s, r, runner, _ := fixture(t)
	v := complete(t, s, r)
	now := runner.now
	s.now = func() time.Time { return now.Add(25 * time.Hour) }
	got, err := s.Get(v.WebsiteID, v.Site)
	if err != nil || got.Report != nil || got.ExpiresAt != v.ExpiresAt || got.Status != "complete" {
		t.Fatal(got, err)
	}
	got, err = s.SetEnabled(v.WebsiteID, v.Site, 1, false)
	if err != nil || got.Enabled || got.NextSyncAt != "" {
		t.Fatal(got, err)
	}
	s.now = func() time.Time { return now.Add(49 * time.Hour) }
	s.claim()
	if s.repo.read().Entries[0].Report != nil {
		t.Fatal("retentionnotpurged")
	}
	r.record.Website.Revision++
	got, err = s.Get(v.WebsiteID, v.Site)
	if err != nil || got.Status != "idle" || got.Report != nil {
		t.Fatal(got, err)
	}
}

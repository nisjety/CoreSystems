package webvitals

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type testRegistry struct{ record websites.Record }

func (r *testRegistry) Resolve(id, site string) (websites.Record, error) {
	if r.record.Website.ID != id || r.record.Website.Site != site {
		return websites.Record{}, websites.ErrNotFound
	}
	return r.record, nil
}
func (*testRegistry) OrganizationID() string { return "org_test" }

type testCollector struct {
	data  Export
	calls int
	err   error
}

func (c *testCollector) Collect(context.Context, websites.Record, string) (Export, error) {
	c.calls++
	return c.data, c.err
}
func fixture(t *testing.T) (*Service, *testRegistry, *testCollector, string) {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "web-vitals.json")
	now := time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC)
	r := &testRegistry{websites.Record{Website: websites.Website{ID: "web_00000000000000000000000000000001", Site: "https://triodelab.no/", Revision: 7, Ownership: websites.Ownership{Status: "google_owner"}}}}
	w := r.record.Website
	c := &testCollector{data: Export{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Environment: "production", ReportingOrigin: "https://www.triodelab.no", Status: "connected", ObservedAt: now.Format(time.RFC3339Nano), Samples: []Sample{}}}
	s, err := New(path, "org_test", r, c, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	return s, r, c, path
}
func sample(index int, value float64, now string) Sample {
	return Sample{SampleID: []string{"00000000-0000-4000-8000-000000000001", "00000000-0000-4000-8000-000000000002", "00000000-0000-4000-8000-000000000003", "00000000-0000-4000-8000-000000000004"}[index], Sequence: 1, Metric: "LCP", Value: value, Viewport: "wide", Path: "/innsikt", ReceivedAt: now}
}
func TestPullAggregationPersistenceAndReadPurity(t *testing.T) {
	s, r, c, path := fixture(t)
	w := r.record.Website
	before, err := s.Get(w.ID, w.Site, "production")
	if err != nil || before.Status != "not_configured" || c.calls != 0 {
		t.Fatal(before, err)
	}
	for i, v := range []float64{10, 40, 20, 30} {
		c.data.Samples = append(c.data.Samples, sample(i, v, c.data.ObservedAt))
	}
	got, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production")
	if err != nil || got.Status != "connected" || got.SampleCount != 4 || got.Groups[1].Metrics.LCP.P75 == nil || *got.Groups[1].Metrics.LCP.P75 != 30 {
		t.Fatal(got, err)
	}
	if got.Window.StartDate != "2026-10-17" || got.Window.EndDate != "2026-10-23" || got.Groups[0].Metrics.INP.P75 != nil {
		t.Fatal(got)
	}
	restored, err := New(path, "org_test", r, c, s.now)
	if err != nil {
		t.Fatal(err)
	}
	read, err := restored.Get(w.ID, w.Site, "production")
	if err != nil || read.SampleCount != 4 || c.calls != 1 {
		t.Fatal(read, err, c.calls)
	}
	if read.Environment != "production" || read.Source != "first_party_web_vitals" || read.RetentionDays != 7 {
		t.Fatal(read)
	}
	if _, err = s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production"); !errors.Is(err, ErrCooldown) {
		t.Fatal(err)
	}
	info, _ := os.Stat(path)
	if info.Mode().Perm() != 0600 {
		t.Fatal(info.Mode())
	}
}
func TestSequenceDeduplicationAndIsolation(t *testing.T) {
	s, r, c, _ := fixture(t)
	w := r.record.Website
	first := sample(0, 100, c.data.ObservedAt)
	latest := first
	latest.Sequence = 2
	latest.Value = 200
	c.data.Samples = []Sample{latest, first, latest}
	got, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production")
	if err != nil || got.SampleCount != 1 || *got.Groups[1].Metrics.LCP.P75 != 200 {
		t.Fatal(got, err)
	}
	development, err := s.Get(w.ID, w.Site, "development")
	if err != nil || development.SampleCount != 0 || development.Status != "not_configured" {
		t.Fatal(development, err)
	}
	r.record.Website.Revision++
	stale, err := s.Get(w.ID, w.Site, "production")
	if err != nil || stale.SampleCount != 0 || stale.Status != "not_configured" {
		t.Fatal(stale, err)
	}
	r.record.Website.Ownership.Status = "unverified"
	if got, err := s.Get(w.ID, w.Site, "production"); err != nil || got.SampleCount != 0 {
		t.Fatal(err)
	}
}
func TestExportValidationFailsClosed(t *testing.T) {
	for _, change := range []func(*Export){
		func(e *Export) { e.ReportingOrigin = "https://evil.no" }, func(e *Export) { e.WebsiteRevision++ }, func(e *Export) { e.Environment = "development" },
		func(e *Export) { e.Samples[0].Path = "/secret?token=abc" }, func(e *Export) { e.Samples[0].Value = 120001 }, func(e *Export) { e.Samples[0].Sequence = 33 },
		func(e *Export) { e.Samples[0].ReceivedAt = "2026-10-24T00:00:00Z" }, func(e *Export) { e.Samples[0].Viewport = "mobile" },
		func(e *Export) { e.Samples[0].Metric = "FID" }, func(e *Export) { e.Samples[0].SampleID = "not-a-uuid" },
		func(e *Export) {
			other := e.Samples[0]
			other.Path = "/other"
			other.Sequence = 2
			e.Samples = append(e.Samples, other)
		},
		func(e *Export) { e.Status = "not_configured" }, func(e *Export) { e.ObservedAt = "2026-10-24T00:00:00Z" },
	} {
		s, r, c, _ := fixture(t)
		w := r.record.Website
		c.data.Samples = []Sample{sample(0, 100, c.data.ObservedAt)}
		change(&c.data)
		if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production"); !errors.Is(err, ErrInvalid) {
			t.Fatal(c.data, err)
		}
		got, _ := s.Get(w.ID, w.Site, "production")
		if got.SampleCount != 0 {
			t.Fatal(got)
		}
	}
}
func TestRetentionAndEmptyConfiguredReport(t *testing.T) {
	s, r, c, _ := fixture(t)
	w := r.record.Website
	empty, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production")
	if err != nil || empty.Status != "awaiting_samples" {
		t.Fatal(empty, err)
	}
	s.now = func() time.Time { return time.Date(2026, 10, 24, 12, 0, 0, 0, time.UTC) }
	c.data.ObservedAt = s.now().Format(time.RFC3339Nano)
	c.data.Samples = []Sample{sample(0, 0, "2026-10-18T00:00:00Z")}
	c.data.Samples[0].Metric = "CLS"
	got, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production")
	if err != nil || got.SampleCount != 1 || got.Groups[1].Metrics.CLS.P75 == nil || *got.Groups[1].Metrics.CLS.P75 != 0 {
		t.Fatal(got, err)
	}
	s.now = func() time.Time { return time.Date(2026, 10, 25, 12, 0, 0, 0, time.UTC) }
	got, err = s.Get(w.ID, w.Site, "production")
	if err != nil || got.SampleCount != 0 || got.Status != "awaiting_samples" {
		t.Fatal(got, err)
	}
}
func TestLaterPullCannotRegressAnExistingMeasurement(t *testing.T) {
	s, r, c, _ := fixture(t)
	w := r.record.Website
	latest := sample(0, 300, c.data.ObservedAt)
	latest.Sequence = 3
	c.data.Samples = []Sample{latest}
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production"); err != nil {
		t.Fatal(err)
	}
	s.now = func() time.Time { return time.Date(2026, 10, 23, 13, 0, 0, 0, time.UTC) }
	c.data.ObservedAt = stamp(s.now())
	old := latest
	old.Sequence = 1
	old.Value = 100
	c.data.Samples = []Sample{old}
	got, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production")
	if err != nil || got.SampleCount != 1 || *got.Groups[1].Metrics.LCP.P75 != 300 {
		t.Fatal(got, err)
	}
}

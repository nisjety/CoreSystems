package siteaudit

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

const testSite = "https://triodelab.no/"
const testID = "web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

var testNow = time.Date(2026, 10, 22, 12, 0, 0, 0, time.UTC)

type registry struct {
	mu       sync.Mutex
	revision int64
}

func (r *registry) OrganizationID() string { return "org-one" }
func (r *registry) Resolve(id, site string) (websites.Record, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if id != testID {
		return websites.Record{}, websites.ErrNotFound
	}
	if site != testSite {
		return websites.Record{}, websites.ErrConflict
	}
	return websites.Record{Website: websites.Website{ID: testID, Site: testSite, Revision: r.revision}}, nil
}

type fakeCapture struct {
	mapFn     func(context.Context, string) ([]discovery.Page, error)
	captureFn func(context.Context, string, string) (CapturePage, error)
}

func (f fakeCapture) Map(ctx context.Context, site string) ([]discovery.Page, error) {
	return f.mapFn(ctx, site)
}
func (f fakeCapture) Capture(ctx context.Context, site, raw string) (CapturePage, error) {
	return f.captureFn(ctx, site, raw)
}
func source() fakeCapture {
	return fakeCapture{mapFn: func(context.Context, string) ([]discovery.Page, error) {
		return []discovery.Page{{URL: testSite}, {URL: testSite + "missing"}}, nil
	}, captureFn: func(_ context.Context, _, raw string) (CapturePage, error) {
		p := testPage(raw, 200, "")
		p.ObservedAt = stamp(testNow)
		return CapturePage{URL: p.URL, FinalURL: p.FinalURL, Title: p.Title, Description: p.Description, Canonical: p.Canonical, ContentType: "text/html", ObservedAt: p.ObservedAt, RunID: p.RunID, Fingerprint: p.Fingerprint, HTTPStatus: p.HTTPStatus, Links: p.Links}, nil
	}}
}
func testService(t *testing.T, c Capture) (*Service, *Repository, *registry, string) {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "audits.json")
	repo, err := Open(path, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	r := &registry{revision: 1}
	return New(repo, r, c, func() time.Time { return testNow }), repo, r, path
}
func finish(t *testing.T, s *Service) Snapshot {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		v, err := s.Get(testID, testSite)
		if err != nil {
			t.Fatal(err)
		}
		s.mu.Lock()
		active := s.active
		s.mu.Unlock()
		if v.Status != "running" && active == 0 {
			return v
		}
		runtime.Gosched()
	}
	t.Fatal("audit did not finish")
	return Snapshot{}
}
func TestAuditPersistsAuthoritativeFindingsAndRechecks(t *testing.T) {
	s, _, _, path := testService(t, source())
	if v, err := s.Get(testID, testSite); err != nil || v.Status != "idle" {
		t.Fatal(v, err)
	}
	running, err := s.Start(testID, testSite, 1)
	if err != nil || running.Status != "running" {
		t.Fatal(running, err)
	}
	v := finish(t, s)
	if v.Status != "complete" || len(v.Pages) != 2 || len(v.Findings) != 2 {
		t.Fatalf("%+v", v)
	}
	f, err := s.Finding(testID, testSite, 1, v.ID, v.Findings[0].ID)
	if err != nil || f.RuleID != "missing_title_signal" {
		t.Fatal(f, err)
	}
	check, err := s.LatestCheck(testID, testSite, 1, f.PageURL, f.RuleID)
	if err != nil || check.Status != "failed" || check.AuditID != v.ID {
		t.Fatal(check, err)
	}
	check, err = s.Check(testID, testSite, 1, v.ID, f.PageURL, "canonical_target_review")
	if err != nil || check.Status != "passed" {
		t.Fatal(check, err)
	}
	v.Pages[0].Title = "corrupted"
	v.Findings[0].RelatedURLs = append(v.Findings[0].RelatedURLs, "https://bad/")
	copy, _ := s.Get(testID, testSite)
	if copy.Pages[0].Title != "" || len(copy.Findings[0].RelatedURLs) != 0 {
		t.Fatal("mutation leaked")
	}
	repo, err := Open(path, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	restored := New(repo, s.registry, source(), func() time.Time { return testNow })
	got, err := restored.GetRun(testID, testSite, v.ID)
	if err != nil || got.ID != v.ID || len(got.History) != 1 {
		t.Fatal(got, err)
	}
	if _, err := s.Start(testID, testSite, 1); !errors.Is(err, ErrCapacity) {
		t.Fatal("cooldown", err)
	}
}
func TestAuditEnforcesBoundsAndPartialFailure(t *testing.T) {
	c := source()
	active, max := 0, 0
	var mu sync.Mutex
	c.mapFn = func(context.Context, string) ([]discovery.Page, error) {
		pages := []discovery.Page{{URL: testSite + "login"}, {URL: "https://other.no/"}}
		for i := range Limit + 20 {
			pages = append(pages, discovery.Page{URL: fmt.Sprintf("%sp%d", testSite, i)})
		}
		return pages, nil
	}
	base := c.captureFn
	c.captureFn = func(ctx context.Context, site, raw string) (CapturePage, error) {
		mu.Lock()
		active++
		if active > max {
			max = active
		}
		mu.Unlock()
		defer func() { mu.Lock(); active--; mu.Unlock() }()
		if raw == testSite+"p0" {
			return CapturePage{}, &CaptureError{Reason: "policy_blocked"}
		}
		return base(ctx, site, raw)
	}
	s, _, _, _ := testService(t, c)
	if _, err := s.Start(testID, testSite, 1); err != nil {
		t.Fatal(err)
	}
	v := finish(t, s)
	if v.Status != "complete" || len(v.Pages) != Limit || max > 2 {
		t.Fatal(v.Status, len(v.Pages), max)
	}
	failed := 0
	for _, p := range v.Pages {
		if p.Status == "failed" {
			failed++
			if len(p.Checks) != 0 {
				t.Fatal("failed page got checks")
			}
		}
	}
	if failed != 1 {
		t.Fatal("missing partial failure", failed)
	}
}
func TestAuditFailClosedBindingSourceAndPersistence(t *testing.T) {
	s, _, r, _ := testService(t, nil)
	if v, err := s.Start(testID, testSite, 1); err != nil || v.Reason != "not_configured" {
		t.Fatal(v, err)
	}
	for _, tc := range []struct {
		id, site string
		rev      int64
	}{{"bad", testSite, 1}, {testID, "https://other.no/", 1}, {testID, testSite, 0}, {testID, testSite, 2}} {
		if _, err := s.Start(tc.id, tc.site, tc.rev); err == nil {
			t.Fatal("bad binding accepted")
		}
	}
	c := source()
	block := make(chan struct{})
	c.mapFn = func(context.Context, string) ([]discovery.Page, error) {
		<-block
		return []discovery.Page{{URL: testSite}}, nil
	}
	s.capture = c
	if _, err := s.Start(testID, testSite, 1); err != nil {
		t.Fatal(err)
	}
	again, err := s.Start(testID, testSite, 1)
	if err != nil || again.Status != "running" {
		t.Fatal("non-idempotent running start")
	}
	r.mu.Lock()
	r.revision = 2
	r.mu.Unlock()
	close(block)
	v := finish(t, s)
	if v.Status != "idle" {
		t.Fatal("stale binding visible", v)
	}
	if _, err := s.Finding(testID, testSite, 2, again.ID, "finding_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"); !errors.Is(err, ErrConflict) {
		t.Fatal("stale run reused", err)
	}
}
func TestAuditRejectsMalformedCaptureAndMapFailure(t *testing.T) {
	for _, reason := range []string{"invalid", "map"} {
		t.Run(reason, func(t *testing.T) {
			c := source()
			if reason == "map" {
				c.mapFn = func(context.Context, string) ([]discovery.Page, error) {
					return nil, &discovery.CaptureError{Reason: "quota_limited"}
				}
			} else {
				c.captureFn = func(context.Context, string, string) (CapturePage, error) {
					return CapturePage{URL: "https://evil.test/"}, nil
				}
			}
			s, _, _, _ := testService(t, c)
			_, _ = s.Start(testID, testSite, 1)
			v := finish(t, s)
			if v.Status != "failed" || len(v.Findings) != 0 {
				t.Fatal(v)
			}
			if _, err := s.LatestCheck(testID, testSite, 1, testSite, "http_error"); !errors.Is(err, ErrUnavailable) {
				t.Fatal(err)
			}
		})
	}
}

func TestLatestCheckDoesNotFallBackPastANewerFailure(t *testing.T) {
	s, _, _, _ := testService(t, source())
	if _, err := s.Start(testID, testSite, 1); err != nil {
		t.Fatal(err)
	}
	finish(t, s)
	s.now = func() time.Time { return testNow.Add(2 * time.Minute) }
	c := source()
	c.captureFn = func(context.Context, string, string) (CapturePage, error) {
		return CapturePage{}, &CaptureError{Reason: "policy_blocked"}
	}
	s.capture = c
	if _, err := s.Start(testID, testSite, 1); err != nil {
		t.Fatal(err)
	}
	finish(t, s)
	if _, err := s.LatestCheck(testID, testSite, 1, testSite, "http_error"); !errors.Is(err, ErrUnavailable) {
		t.Fatal("older passing observation used after newer failure", err)
	}
}

func TestAuditRejectsStaleCaptureProofInsideNewReceipt(t *testing.T) {
	c := source()
	base := c.captureFn
	c.captureFn = func(ctx context.Context, site, raw string) (CapturePage, error) {
		value, err := base(ctx, site, raw)
		value.ObservedAt = stamp(testNow.Add(-time.Hour))
		return value, err
	}
	s, _, _, _ := testService(t, c)
	if _, err := s.Start(testID, testSite, 1); err != nil {
		t.Fatal(err)
	}
	v := finish(t, s)
	if v.Status != "failed" || len(v.Findings) != 0 {
		t.Fatal("old proof appeared under new audit", v)
	}
	for _, p := range v.Pages {
		if p.Status != "failed" || p.Reason != "invalid_source" {
			t.Fatal("stale capture not explicitly rejected", p)
		}
	}
}

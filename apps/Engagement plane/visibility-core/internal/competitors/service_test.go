package competitors

import (
	"context"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

const testSite = "https://triodelab.no/"
const testTarget = "https://competitor.no/"
const testID = "web_11111111111111111111111111111111"

type registry struct {
	revision int64
	org      string
}

func (r *registry) OrganizationID() string { return r.org }
func (r *registry) Resolve(id, site string) (websites.Record, error) {
	if id != testID || site != testSite {
		return websites.Record{}, websites.ErrNotFound
	}
	return websites.Record{Website: websites.Website{ID: id, Site: site, Revision: r.revision}}, nil
}

type capture struct {
	now   *time.Time
	calls int
	fail  bool
	after func()
}

func (c *capture) Capture(_ context.Context, site, raw string) (siteaudit.CapturePage, error) {
	c.calls++
	if c.after != nil {
		c.after()
	}
	if c.fail {
		return siteaudit.CapturePage{}, errors.New("upstream failed")
	}
	return siteaudit.CapturePage{URL: raw, FinalURL: raw, HTTPStatus: 200, Title: "Observed title", Description: "Observed description", Canonical: raw, ContentType: "text/html", ObservedAt: stamp(*c.now), RunID: "run_01" + strings.Repeat("0", 24), Fingerprint: "blake3:" + strings.Repeat("a", 64), Links: []string{site + "about"}}, nil
}
func fixture(t *testing.T) (*Service, *Repository, *registry, *capture, *time.Time) {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := Open(filepath.Join(dir, "comparisons.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 10, 28, 12, 0, 0, 0, time.UTC)
	r := &registry{revision: 1, org: "org-test"}
	c := &capture{now: &now}
	return New(repo, r, c, func() time.Time { return now }), repo, r, c, &now
}
func complete(t *testing.T, s *Service) {
	t.Helper()
	job, ok := s.claim()
	if !ok {
		t.Fatal("no persisted claim")
	}
	s.collect(t.Context(), job)
}
func TestPublicComparisonPersistsAndGetNeverCollects(t *testing.T) {
	s, repo, r, c, now := fixture(t)
	v, err := s.Command(testID, testSite, 1, testTarget, "refresh")
	if err != nil || v.Entries[0].Status != "queued" || c.calls != 0 {
		t.Fatal(v, err)
	}
	complete(t, s)
	v, err = s.Get(testID, testSite)
	if err != nil || v.Entries[0].Status != "complete" || v.Entries[0].Competitor.Title != "Observed title" || c.calls != 2 {
		t.Fatal(v, err)
	}
	reopened, err := Open(repo.path, "org-test")
	if err != nil {
		t.Fatal(err)
	}
	v, err = New(reopened, r, nil, func() time.Time { return *now }).Get(testID, testSite)
	if err != nil || v.Entries[0].Own == nil {
		t.Fatal(v, err)
	}
	*now = now.Add(24 * time.Hour)
	c.fail = true
	complete(t, s)
	v, _ = s.Get(testID, testSite)
	if v.Entries[0].Status != "failed" || v.Entries[0].Competitor == nil {
		t.Fatal("lost previous success", v)
	}
	if _, ok := s.claim(); ok {
		t.Fatal("ignored failure backoff")
	}
}

func TestChangedBindingBeforeCollectionNeverFetches(t *testing.T) {
	s, _, r, c, _ := fixture(t)
	if _, err := s.Command(testID, testSite, 1, testTarget, "refresh"); err != nil {
		t.Fatal(err)
	}
	job, ok := s.claim()
	if !ok {
		t.Fatal("missing claim")
	}
	r.revision++
	s.collect(t.Context(), job)
	if c.calls != 0 {
		t.Fatal("fetched after website revision changed")
	}
}

func TestPersistedEvidenceRejectsUnsafeCanonicalAndMissingAttempt(t *testing.T) {
	s, repo, _, _, _ := fixture(t)
	if _, err := s.Command(testID, testSite, 1, testTarget, "refresh"); err != nil {
		t.Fatal(err)
	}
	complete(t, s)
	v := repo.read()
	v.Entries[0].Own.Canonical = "https://other.no/"
	if validState(v) == nil {
		t.Fatal("accepted unsafe canonical")
	}
	v = repo.read()
	v.Entries[0].LastAttemptAt = ""
	if validState(v) == nil {
		t.Fatal("accepted completed evidence without an attempt")
	}
}
func TestScopesTargetsLimitAndFailedClaims(t *testing.T) {
	s, repo, r, c, _ := fixture(t)
	for _, target := range []string{testSite, "https://www.triodelab.no/", "http://competitor.no/", "https://localhost/", "https://127.0.0.1/", testTarget + "page", "https://user:secret@competitor.no/", "https://competitor.no/?secret=x"} {
		if _, err := s.Command(testID, testSite, 1, target, "refresh"); err == nil {
			t.Fatal("unsafe target", target)
		}
	}
	for _, target := range []string{testTarget, "https://second.no/", "https://third.no/"} {
		if _, err := s.Command(testID, testSite, 1, target, "refresh"); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := s.Command(testID, testSite, 1, "https://fourth.no/", "refresh"); !errors.Is(err, ErrCapacity) {
		t.Fatal(err)
	}
	repo.persistFn = func(diskState) error { return errors.New("disk full") }
	if _, ok := s.claim(); ok {
		t.Fatal("unpersisted claim")
	}
	if c.calls != 0 {
		t.Fatal("unexpected network")
	}
	r.revision++
	v, err := s.Get(testID, testSite)
	if err != nil || len(v.Entries) != 0 {
		t.Fatal("old revision visible", v, err)
	}
	if _, err := Open(repo.path, "wrong-org"); err == nil {
		t.Fatal("cross-org file read")
	}
}
func TestRemovedInFlightComparisonCannotReappear(t *testing.T) {
	s, _, _, c, _ := fixture(t)
	s.Command(testID, testSite, 1, testTarget, "refresh")
	job, ok := s.claim()
	if !ok {
		t.Fatal("claim")
	}
	c.after = func() {
		c.after = nil
		s.Command(testID, testSite, 1, testTarget, "remove")
		s.Command(testID, testSite, 1, testTarget, "refresh")
	}
	s.collect(t.Context(), job)
	v, _ := s.Get(testID, testSite)
	if len(v.Entries) != 1 || v.Entries[0].Status != "queued" || v.Entries[0].Competitor != nil {
		t.Fatal("old job published into new registration", v)
	}
}
func TestRevisionChangeStopsFurtherFetchesAndPreventsPublishing(t *testing.T) {
	s, _, r, c, _ := fixture(t)
	s.Command(testID, testSite, 1, testTarget, "refresh")
	job, _ := s.claim()
	c.after = func() { r.revision++ }
	s.collect(t.Context(), job)
	v, _ := s.Get(testID, testSite)
	if len(v.Entries) != 0 || c.calls != 1 {
		t.Fatal("continued outdated context", v, c.calls)
	}
}

func TestSelectedPagePairsRetainMeasuredHistoryAndRespectLimits(t *testing.T) {
	s, repo, r, c, now := fixture(t)
	_, err := s.Command(testID, testSite, 1, testTarget, "refresh", testSite+"services", testTarget+"services")
	if err != nil {
		t.Fatal(err)
	}
	complete(t, s)
	v, _ := s.Get(testID, testSite)
	e := v.Entries[0]
	if e.Own.URL != testSite+"services" || e.Competitor.URL != testTarget+"services" || len(e.History) != 1 || e.History[0].Changed {
		t.Fatal(e)
	}
	*now = now.Add(24 * time.Hour)
	complete(t, s)
	v, _ = s.Get(testID, testSite)
	if len(v.Entries[0].History) != 2 || v.Entries[0].History[0].Changed {
		t.Fatal(v)
	}
	reopened, err := Open(repo.path, "org-test")
	if err != nil {
		t.Fatal(err)
	}
	s = New(reopened, r, c, func() time.Time { return *now })
	for _, path := range []string{"a", "b", "c", "d"} {
		if _, err = s.Command(testID, testSite, 1, testTarget, "refresh", testSite+path, testTarget+path); err != nil {
			t.Fatal(err)
		}
	}
	if _, err = s.Command(testID, testSite, 1, testTarget, "refresh", testSite+"e", testTarget+"e"); !errors.Is(err, ErrCapacity) {
		t.Fatal(err)
	}
	if _, err = s.Command(testID, testSite, 1, testTarget, "refresh", testSite+"admin", testTarget); !errors.Is(err, ErrInvalid) {
		t.Fatal(err)
	}
}

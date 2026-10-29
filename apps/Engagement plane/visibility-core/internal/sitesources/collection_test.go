package sitesources

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type collectionAuthority struct {
	*fakeAuthority
	bindings []Enrollment
	mu       sync.Mutex
}

func (a *collectionAuthority) Enrollments() []Enrollment {
	a.mu.Lock()
	defer a.mu.Unlock()
	return append([]Enrollment{}, a.bindings...)
}
func (a *collectionAuthority) Authorize(ctx context.Context, r websites.Record) (Authorization, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.fakeAuthority.Authorize(ctx, r)
}
func (a *collectionAuthority) CollectOAuth(ctx context.Context, r websites.Record, b Authorization) (Report, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.fakeAuthority.CollectOAuth(ctx, r, b)
}
func (a *collectionAuthority) BindSnapshot(r websites.Record, b Authorization, report Report) error {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.fakeAuthority.BindSnapshot(r, b, report)
}
func collectionFixture(t *testing.T) (*Service, *fakeRegistry, *collectionAuthority, string) {
	t.Helper()
	_, r, c, path := fixture(t)
	w := r.record.Website
	c.report.Algolia.Region = "provider"
	c.report.Algolia.TopSearches = &Terms{Status: "connected", Limit: 20, Rows: []Term{}}
	c.report.Algolia.NoResultTerms = &Terms{Status: "connected", Limit: 20, Rows: []Term{}}
	a := &collectionAuthority{fakeAuthority: &fakeAuthority{authorization: Authorization{Managed: true, Active: true, Fingerprint: strings.Repeat("a", 64), Generation: "grant-1"}, report: c.report}, bindings: []Enrollment{{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Fingerprint: strings.Repeat("a", 64)}}}
	s, err := NewWithOAuth(path, "org_sites", r, c, a, func() time.Time { return time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC) })
	if err != nil {
		t.Fatal(err)
	}
	return s, r, a, path
}

func TestCollectionQueueIsDurableReadOnlyAndProducesReadySnapshot(t *testing.T) {
	s, r, a, path := collectionFixture(t)
	w := r.record.Website
	state, err := s.Collection(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || state.Status != "idle" || a.calls != 0 {
		t.Fatal(state, err)
	}
	state, err = s.QueueCollection(w.ID, w.Site, w.Revision, "algolia")
	if err != nil || state.Status != "queued" || a.calls != 0 {
		t.Fatal(state, err)
	}
	for range 3 {
		if _, err = s.Collection(t.Context(), w.ID, w.Site, "algolia"); err != nil {
			t.Fatal(err)
		}
	}
	if a.calls != 0 {
		t.Fatal("GET collected provider data")
	}
	restored, err := NewWithOAuth(path, "org_sites", r, nil, a, s.now)
	if err != nil {
		t.Fatal(err)
	}
	job, ok := restored.claimCollection()
	if !ok {
		t.Fatal("durable queued job missing")
	}
	restored.executeCollection(t.Context(), job)
	state, err = restored.Collection(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || state.Status != "ready" || a.calls != 1 || state.NextRunAt != "2026-10-23T18:00:00Z" {
		t.Fatal(state, err, a.calls)
	}
	report, err := restored.GetContext(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || report.Status != "connected" {
		t.Fatal(report, err)
	}
	info, err := os.Stat(filepath.Join(filepath.Dir(path), "site-source-jobs.json"))
	if err != nil || info.Mode().Perm() != 0600 {
		t.Fatal(info, err)
	}
}

func TestCollectionRecoversMappingAndHonorsRevocationAndContextChanges(t *testing.T) {
	s, r, a, _ := collectionFixture(t)
	w := r.record.Website
	if err := s.reconcileCollections(); err != nil {
		t.Fatal(err)
	}
	job, ok := s.claimCollection()
	if !ok {
		t.Fatal("mapping did not enroll first sync")
	}
	a.authorization.Active = false
	s.executeCollection(t.Context(), job)
	state, err := s.Collection(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || state.Status != "reconnect_required" || a.calls != 0 {
		t.Fatal(state, err)
	}
	a.bindings = nil
	state, err = s.Collection(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || state.Status != "idle" || a.calls != 0 {
		t.Fatal("unmapped lifecycle exposed", state, err)
	}
	if _, err = s.QueueCollection(w.ID, w.Site, w.Revision, "algolia"); !errors.Is(err, ErrInvalid) {
		t.Fatal(err)
	}
	if _, err = s.QueueCollection(w.ID, "https://other.no/", w.Revision, "algolia"); err == nil {
		t.Fatal("wrong site queued")
	}
	if _, err = s.QueueCollection(w.ID, w.Site, w.Revision+1, "algolia"); err == nil {
		t.Fatal("wrong revision queued")
	}
}

func TestCollectionPartialReportRetriesWithoutHidingValidTerms(t *testing.T) {
	s, r, a, _ := collectionFixture(t)
	w := r.record.Website
	a.report.Algolia.NoResultTerms = &Terms{Status: "unavailable", Limit: 20}
	if _, err := s.QueueCollection(w.ID, w.Site, w.Revision, "algolia"); err != nil {
		t.Fatal(err)
	}
	job, _ := s.claimCollection()
	s.executeCollection(t.Context(), job)
	state, err := s.Collection(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || state.Status != "needs_attention" || state.Reason != "partial_report" || state.LastSuccessAt == "" {
		t.Fatal(state, err)
	}
	report, err := s.GetContext(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || !connectedTerms(report.Algolia.TopSearches) {
		t.Fatal(report, err)
	}
}

func TestCollectionRunsJoinedAndRecoversInterruptedState(t *testing.T) {
	s, _, _, _ := collectionFixture(t)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	done := make(chan struct{})
	go func() { s.Run(ctx); close(done) }()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("collection lifecycle did not join")
	}
}

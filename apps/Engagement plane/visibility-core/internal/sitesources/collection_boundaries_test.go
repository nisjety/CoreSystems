package sitesources

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestCollectionRunJoinsWorkersAndCollectsOnce(t *testing.T) {
	s, r, a, _ := collectionFixture(t)
	w := r.record.Website
	if _, err := s.QueueCollection(w.ID, w.Site, w.Revision, "algolia"); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	done := make(chan struct{})
	go func() { s.Run(ctx); close(done) }()
	deadline := time.Now().Add(5 * time.Second)
	for {
		state, err := s.Collection(t.Context(), w.ID, w.Site, "algolia")
		if err != nil {
			cancel()
			<-done
			t.Fatal(err)
		}
		if state.Status == "ready" {
			break
		}
		if time.Now().After(deadline) {
			cancel()
			<-done
			t.Fatal(state)
		}
		time.Sleep(time.Millisecond * 5)
	}
	cancel()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("workers were not joined")
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.calls != 1 {
		t.Fatal(a.calls)
	}
}

func TestCollectionEntropyRejectsFailureShortAndZeroReads(t *testing.T) {
	for _, reader := range []func([]byte) (int, error){func([]byte) (int, error) { return 0, errors.New("entropy failed") }, func([]byte) (int, error) { return 31, nil }, func([]byte) (int, error) { return 32, nil }} {
		if _, err := newCollectionID(reader); !errors.Is(err, ErrUnavailable) {
			t.Fatal(err)
		}
	}
	id, err := newCollectionID(func(b []byte) (int, error) {
		for i := range b {
			b[i] = 1
		}
		return len(b), nil
	})
	if err != nil || id != strings.Repeat("01", 32) {
		t.Fatal(id, err)
	}
}

func TestCollectionStorageFailureIsVisibleAndRecoverable(t *testing.T) {
	s, r, _, path := collectionFixture(t)
	w := r.record.Website
	dir := filepath.Dir(path)
	if err := os.Chmod(dir, 0755); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chmod(dir, 0700) })
	if _, err := s.QueueCollection(w.ID, w.Site, w.Revision, "algolia"); err == nil {
		t.Fatal("unsafe storage accepted")
	}
	state, err := s.Collection(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || state.Status != "needs_attention" || state.Reason != "provider_unavailable" {
		t.Fatal(state, err)
	}
	if err = os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	if err = s.reconcileCollections(); err != nil {
		t.Fatal(err)
	}
	state, err = s.Collection(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || state.Status != "queued" {
		t.Fatal(state, err)
	}
}

func TestCollectionRemapCancelsOldRunAndCannotCompleteIt(t *testing.T) {
	s, r, a, _ := collectionFixture(t)
	w := r.record.Website
	if err := s.reconcileCollections(); err != nil {
		t.Fatal(err)
	}
	job, ok := s.claimCollection()
	if !ok {
		t.Fatal("missing job")
	}
	canceled := false
	s.collectionCancels[job.RunID] = func() { canceled = true }
	next := a.bindings[0]
	next.Fingerprint = strings.Repeat("b", 64)
	a.bindings = []Enrollment{next}
	a.authorization.Fingerprint = next.Fingerprint
	if err := s.reconcileCollections(); err != nil {
		t.Fatal(err)
	}
	if !canceled {
		t.Fatal("old run not canceled")
	}
	s.finishCollection(job, a.report, "")
	state, err := s.Collection(t.Context(), w.ID, w.Site, "algolia")
	if err != nil || state.Status != "queued" || len(s.collections.state.Entries) != 1 || s.collections.state.Entries[0].Fingerprint != next.Fingerprint {
		t.Fatal(state, err)
	}
}

func TestCollectionSchedulingAndFailureBackoff(t *testing.T) {
	s, r, a, _ := collectionFixture(t)
	w := r.record.Website
	now := s.now()
	for _, tt := range []struct {
		observed string
		want     time.Time
	}{{stamp(now), now.Add(6 * time.Hour)}, {stamp(now.Add(-7 * time.Hour)), now.Add(time.Minute)}, {"2026-10-23T22:00:00Z", time.Date(2026, 10, 24, 0, 0, 0, 0, time.UTC)}} {
		if got := nextCollection(now, Report{ObservedAt: tt.observed}); !got.Equal(tt.want) {
			t.Fatal(got, tt.want)
		}
	}
	for _, tt := range []struct {
		attempts       int
		why            string
		report         Report
		status, reason string
		delay          time.Duration
	}{{1, "provider_unavailable", Report{}, "needs_attention", "provider_unavailable", time.Minute}, {2, "provider_unavailable", Report{}, "needs_attention", "provider_unavailable", 5 * time.Minute}, {3, "provider_unavailable", Report{}, "needs_attention", "provider_unavailable", 30 * time.Minute}, {4, "provider_unavailable", Report{}, "needs_attention", "provider_unavailable", time.Hour}, {1, "cooldown", Report{}, "queued", "", time.Minute}, {1, "reconnect_required", Report{}, "reconnect_required", "reconnect_required", time.Hour}, {1, "", Report{Status: "not_configured"}, "needs_attention", "not_configured", time.Minute}, {1, "", Report{Status: "unavailable"}, "needs_attention", "provider_unavailable", time.Minute}, {1, "", Report{Status: "connected", ObservedAt: stamp(now), Algolia: &Algolia{Analytics: a.report.Algolia.Analytics}}, "needs_attention", "partial_report", time.Hour}} {
		if _, err := s.QueueCollection(w.ID, w.Site, w.Revision, "algolia"); err != nil {
			t.Fatal(err)
		}
		j := s.collections.state.Entries[0]
		j.Status = "syncing"
		j.Attempts = tt.attempts
		s.collections.state.Entries = []collectionJob{j}
		s.finishCollection(j, tt.report, tt.why)
		got := s.collections.state.Entries[0]
		if got.Status != tt.status || got.Reason != tt.reason || got.NextRunAt != stamp(now.Add(tt.delay)) {
			t.Fatal(got, tt)
		}
	}
}

func TestCollectionRepositoryRejectsUntrustedMetadata(t *testing.T) {
	s, _, _, _ := collectionFixture(t)
	if err := s.reconcileCollections(); err != nil {
		t.Fatal(err)
	}
	base := s.collections.state
	for _, change := range []func(*collectionDisk){func(d *collectionDisk) { d.Version = 2 }, func(d *collectionDisk) { d.OrganizationID = "org_other" }, func(d *collectionDisk) { d.Entries = nil }, func(d *collectionDisk) { d.Entries = append(d.Entries, d.Entries[0]) }, func(d *collectionDisk) { d.Entries[0].Status = "unknown" }, func(d *collectionDisk) { d.Entries[0].Reason = "raw provider error" }, func(d *collectionDisk) { d.Entries[0].Attempts = 1001 }, func(d *collectionDisk) { d.Entries[0].NextRunAt = "bad date" }, func(d *collectionDisk) { d.Entries[0].Site = "https://example.com/?secret=1" }, func(d *collectionDisk) { d.Entries[0].RunID = "invalid" }} {
		d := base
		d.Entries = append([]collectionJob{}, base.Entries...)
		change(&d)
		if validCollectionDisk(d, base.OrganizationID) {
			t.Fatal("invalid metadata accepted", d)
		}
	}
	if _, err := openCollections("relative.json", "org_sites"); err == nil {
		t.Fatal("relative path accepted")
	}
	if _, err := openCollections(s.collections.path, "org_other"); err == nil {
		t.Fatal("cross-org metadata accepted")
	}
	if err := os.Chmod(s.collections.path, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := openCollections(s.collections.path, "org_sites"); err == nil {
		t.Fatal("public metadata accepted")
	}
	if err := s.collections.save(base.Entries); err == nil {
		t.Fatal("unsafe file overwritten")
	}
}

func TestCollectionReconciliationPhysicallyPrunesOnlyExpiredDerivedSnapshots(t *testing.T) {
	s, r, a, path := collectionFixture(t)
	w := r.record.Website
	a.report.Algolia.TopSearches.Rows = []Term{{Query: "private term", Count: 1}}
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); err != nil {
		t.Fatal(err)
	}
	now := s.now().Add(5*time.Hour + 59*time.Minute)
	s.now = func() time.Time { return now }
	fresh := Report{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Provider: "cookiebot", ObservedAt: stamp(now), Window: period(now), Status: "not_configured"}
	if err := s.store.save(fresh, now); err != nil {
		t.Fatal(err)
	}
	if err := s.reconcileCollections(); err != nil {
		t.Fatal(err)
	}
	if len(s.store.state.Entries) != 2 {
		t.Fatal("unexpired report removed")
	}
	now = now.Add(time.Minute)
	if err := s.reconcileCollections(); err != nil {
		t.Fatal(err)
	}
	if len(s.store.state.Entries) != 1 || s.store.state.Entries[0].Provider != "cookiebot" {
		t.Fatal(s.store.state.Entries)
	}
	raw, err := os.ReadFile(path)
	if err != nil || strings.Contains(string(raw), "private term") {
		t.Fatal("expired term retained", err)
	}
}

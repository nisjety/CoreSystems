package analyticssync

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

func executeQueued(t *testing.T, s *Service, r *fakeRegistry) State {
	t.Helper()
	w := r.record.Website
	if _, err := s.Queue(w.ID, w.Site, w.Revision, "Q4"); err != nil {
		t.Fatal(err)
	}
	e, ok := s.claim()
	if !ok {
		t.Fatal("not claimed")
	}
	s.execute(context.Background(), e)
	v, err := s.Get(w.ID, w.Site, "Q4")
	if err != nil {
		t.Fatal(err)
	}
	return v
}
func TestTTLPeriodicAndNoReadFetch(t *testing.T) {
	s, r, _, runner, _ := fixture(t)
	v := executeQueued(t, s, r)
	now := runner.now
	s.now = func() time.Time { return now.Add(metricsTTL + time.Second) }
	b, err := s.Read(context.Background(), v.WebsiteID, v.Site, "Q4")
	if err != nil || b.Pulse.Totals != nil || b.Pulse.Reason != "snapshot_expired" {
		t.Fatal(b, err)
	}
	e, ok := s.claim()
	if !ok || e.JobID == v.JobID {
		t.Fatal("periodic did not queue fresh job")
	}
	s.execute(context.Background(), e)
	if runner.calls != 2 {
		t.Fatal("periodic collects", runner.calls)
	}
	s.now = func() time.Time { return now.Add(2 * retentionTTL) }
	_, _ = s.SetEnabled(v.WebsiteID, v.Site, v.WebsiteRevision, "Q4", false)
	s.claim()
	for _, entry := range s.repo.read().Entries {
		if entry.Bundle != nil {
			t.Fatal("private payload retained beyond24h")
		}
		if entry.Status == "partial" && entry.ExpiresAt == "" {
			t.Fatal("payload retention erased historical expiry receipt")
		}
	}
	purged, err := s.Read(context.Background(), v.WebsiteID, v.Site, "Q4")
	if err != nil || purged.Pulse.Totals != nil || purged.Pulse.Reason != "snapshot_expired" {
		t.Fatal("purged receipt became pending", purged, err)
	}
}
func TestManualCooldownAndImmutableReads(t *testing.T) {
	s, r, _, _, _ := fixture(t)
	v := executeQueued(t, s, r)
	if _, err := s.Queue(v.WebsiteID, v.Site, v.WebsiteRevision, "Q4"); !errors.Is(err, ErrCooldown) {
		t.Fatal(err)
	}
	b, _ := s.Read(context.Background(), v.WebsiteID, v.Site, "Q4")
	b.Pulse.Totals.Clicks = 999
	b.Pulse.Days[0].Clicks = 999
	v.Steps[0].Reason = "bad"
	fresh, _ := s.Read(context.Background(), v.WebsiteID, v.Site, "Q4")
	if fresh.Pulse.Totals.Clicks != 2 || fresh.Pulse.Days[0].Clicks != 2 {
		t.Fatal("mutable shared data")
	}
}

type blockedRunner struct {
	underlying       *fakeRunner
	entered, release chan struct{}
}

func (b *blockedRunner) Collect(ctx context.Context, r websites.Record, q string) (Bundle, error) {
	close(b.entered)
	select {
	case <-b.release:
		return b.underlying.Collect(ctx, r, q)
	case <-ctx.Done():
		return Bundle{}, ctx.Err()
	}
}
func TestPauseDuringCollectionNotReenabledAndNoRepeatAfterSaveFailure(t *testing.T) {
	s, r, _, runner, _ := fixture(t)
	w := r.record.Website
	block := &blockedRunner{runner, make(chan struct{}), make(chan struct{})}
	s.runner = block
	_, _ = s.Queue(w.ID, w.Site, w.Revision, "Q4")
	e, ok := s.claim()
	if !ok {
		t.Fatal("notclaimed")
	}
	done := make(chan struct{})
	go func() { s.execute(context.Background(), e); close(done) }()
	<-block.entered
	if _, err := s.SetEnabled(w.ID, w.Site, w.Revision, "Q4", false); err != nil {
		t.Fatal(err)
	}
	calls := 0
	original := s.repo.persistFn
	s.repo.persistFn = func(v diskState) error {
		calls++
		if calls == 1 {
			return errors.New("transient disk")
		}
		return original(v)
	}
	close(block.release)
	<-done
	v, _ := s.Get(w.ID, w.Site, "Q4")
	if v.Enabled || v.NextSyncAt != "" || runner.calls != 1 || calls != 2 {
		t.Fatal(v, runner.calls, calls)
	}
}
func TestContextChangesDuringCollectionDiscardPayload(t *testing.T) {
	s, r, _, runner, _ := fixture(t)
	w := r.record.Website
	block := &blockedRunner{runner, make(chan struct{}), make(chan struct{})}
	s.runner = block
	_, _ = s.Queue(w.ID, w.Site, 1, "Q4")
	e, _ := s.claim()
	done := make(chan struct{})
	go func() { s.execute(context.Background(), e); close(done) }()
	<-block.entered
	r.mu.Lock()
	r.record.GSCConnectionID = "new_connection"
	r.mu.Unlock()
	close(block.release)
	<-done
	for _, e := range s.repo.read().Entries {
		if e.Bundle != nil {
			t.Fatal("oldbinding payload retained")
		}
	}
	v, _ := s.Get(w.ID, w.Site, "Q4")
	if v.Status != "idle" {
		t.Fatal(v)
	}
}

type hookAuthority struct {
	fn     func()
	access Access
}

func (a hookAuthority) Check(context.Context, websites.Record) Access { a.fn(); return a.access }
func TestReadRebindsAfterAuthorityAndHidesKnownRevokedClarity(t *testing.T) {
	s, r, _, _, _ := fixture(t)
	v := executeQueued(t, s, r)
	s.authority = hookAuthority{func() { r.mu.Lock(); r.record.Website.Revision++; r.mu.Unlock() }, Access{SearchConsole: true}}
	if _, err := s.Read(context.Background(), v.WebsiteID, v.Site, "Q4"); !errors.Is(err, ErrConflict) {
		t.Fatal(err)
	}
	b := MissingBundle(v.Site, "Q4", s.now(), "not_connected")
	b.Clarity.ProjectID = "project1"
	b.Clarity.BindingVersion = strings.Repeat("a", 64)
	got := mask(b, Access{Clarity: true, ClarityProjectID: "project2", ClarityVersion: strings.Repeat("a", 64)}, v.Site, "Q4", s.now(), "authorization_required")
	if got.Clarity.ProjectID != "" {
		t.Fatal("reboundClarity leaked")
	}
}
func TestRecoveryAttemptLimitAndFailureBackoff(t *testing.T) {
	s, r, _, runner, path := fixture(t)
	w := r.record.Website
	_, _ = s.Queue(w.ID, w.Site, 1, "Q4")
	e, _ := s.claim()
	if err := s.recover(); err != nil {
		t.Fatal(err)
	}
	state, _ := s.Get(w.ID, w.Site, "Q4")
	if state.Status != "queued" || state.StartedAt != "" {
		t.Fatal(state)
	}
	_, _ = s.claim()
	_ = s.recover()
	_, _ = s.claim()
	_ = s.recover()
	state, _ = s.Get(w.ID, w.Site, "Q4")
	if state.Status != "failed" || state.Steps[0].Reason != "interrupted" || parsed(state.NextSyncAt) != s.now().Add(time.Hour) {
		t.Fatal(state)
	}
	if runner.calls != 0 {
		t.Fatal("recover calledprovider")
	}
	if _, err := Open(path, "org_test"); err != nil {
		t.Fatal(err)
	}
	_ = e
}
func TestRepositoryRollbackAndInputGuards(t *testing.T) {
	s, r, _, _, path := fixture(t)
	w := r.record.Website
	s.repo.persistFn = func(diskState) error { return errors.New("disk failed") }
	if _, err := s.Queue(w.ID, w.Site, 1, "Q4"); err == nil {
		t.Fatal("writefail swallowed")
	}
	if len(s.repo.read().Entries) != 0 {
		t.Fatal("failedwrite mutatedstate")
	}
	for _, q := range []string{"", "q4", "Q5", "Q4&org=x"} {
		if _, err := s.Queue(w.ID, w.Site, 1, q); err == nil {
			t.Fatal("badquarter", q)
		}
	}
	for _, bad := range []string{filepath.Join(filepath.Dir(path), "missing", "x.json"), "relative.json"} {
		if _, err := Open(bad, "org_test"); err == nil {
			t.Fatal("badpath")
		}
	}
	if err := os.WriteFile(path, []byte(`{"version":1,"organizationId":"org_test","entries":[],"secret":"no"}`), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(path, "org_test"); err == nil {
		t.Fatal("unknownpersistence accepted")
	}
}
func TestTwoWorkersAndCancellationJoined(t *testing.T) {
	s, _, _, _, _ := fixture(t)
	ctx, cancel := context.WithCancel(context.Background())
	var wg sync.WaitGroup
	wg.Add(1)
	go func() { defer wg.Done(); s.Run(ctx) }()
	time.Sleep(time.Millisecond)
	s.Run(ctx)
	cancel()
	wg.Wait()
}

func TestQueueIdempotentEnablePauseAndFailure(t *testing.T) {
	s, r, a, runner, _ := fixture(t)
	w := r.record.Website
	idle, err := s.SetEnabled(w.ID, w.Site, 1, "Q4", false)
	if err != nil || idle.Status != "idle" {
		t.Fatal(idle, err)
	}
	queued, err := s.SetEnabled(w.ID, w.Site, 1, "Q4", true)
	if err != nil {
		t.Fatal(err)
	}
	again, err := s.Queue(w.ID, w.Site, 1, "Q4")
	if err != nil || again.JobID != queued.JobID {
		t.Fatal("duplicatequeued", err)
	}
	_, _ = s.SetEnabled(w.ID, w.Site, 1, "Q4", false)
	if _, ok := s.claim(); ok {
		t.Fatal("pausedqueuedexecuted")
	}
	_, _ = s.SetEnabled(w.ID, w.Site, 1, "Q4", true)
	e, ok := s.claim()
	if !ok {
		t.Fatal("resumednotclaimed")
	}
	a.mu.Lock()
	a.access = Access{}
	a.mu.Unlock()
	s.execute(context.Background(), e)
	v, _ := s.Get(w.ID, w.Site, "Q4")
	if v.Status != "failed" || runner.calls != 0 {
		t.Fatal(v, runner.calls)
	}
	_, _ = s.SetEnabled(w.ID, w.Site, 1, "Q4", false)
	v, err = s.SetEnabled(w.ID, w.Site, 1, "Q4", true)
	if err != nil || v.NextSyncAt == "" {
		t.Fatal(v, err)
	}
	s.authority = nil
	if _, err = s.Read(context.Background(), w.ID, w.Site, "Q4"); err != nil {
		t.Fatal(err)
	}
	_ = New(nil, nil, nil, nil, nil)
}

func TestRepositoryBudgetPrunesOnlyOldTerminal(t *testing.T) {
	s, r, _, _, _ := fixture(t)
	_ = executeQueued(t, s, r)
	base := s.repo.read().Entries[0]
	prior := diskState{Version: 1, OrganizationID: "org_test", Entries: []entry{}}
	for i := range 4 {
		e := clone(base)
		e.JobID = string(rune('a' + i))
		e.Bundle.Search.Queries = []pulse.SearchRow{{Key: strings.Repeat("x", 3<<20)}}
		prior.Entries = append(prior.Entries, e)
	}
	next := clone(prior)
	next.Entries[3].Status = "running"
	next.Entries[3].Bundle = nil
	got, err := trimBudget(next, prior)
	if err != nil {
		t.Fatal(err)
	}
	if len(got.Entries) >= 4 {
		t.Fatal("budgetnottrimmed")
	}
	found := false
	for _, e := range got.Entries {
		if e.JobID == next.Entries[3].JobID {
			found = true
		}
	}
	if !found {
		t.Fatal("inflightpruned")
	}
	for i := range next.Entries {
		next.Entries[i].Status = "running"
	}
	if _, err = trimBudget(next, diskState{}); !errors.Is(err, ErrCapacity) {
		t.Fatal(err)
	}
}

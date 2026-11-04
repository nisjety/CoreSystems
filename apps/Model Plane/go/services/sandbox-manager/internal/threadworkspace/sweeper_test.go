package threadworkspace

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"
)

// fakeIdle is an idleEraser over an in-memory set of threads. A thread listed in
// touched was read or applied after the sweep chose it, so EraseIdle keeps it.
type fakeIdle struct {
	mu           sync.Mutex
	threads      []ThreadKey
	touched      map[ThreadKey]bool
	shared       map[ThreadKey]bool
	listErr      error
	files        int64
	erased       []ThreadKey
	cutoffs      []time.Time
	limits       []int
	ticks        chan struct{}
	pruneErr     error
	pruneCutoffs []time.Time
}

func newFakeIdle(threads ...ThreadKey) *fakeIdle {
	return &fakeIdle{threads: threads, touched: map[ThreadKey]bool{}, ticks: make(chan struct{}, 64)}
}

func (f *fakeIdle) ListIdle(_ context.Context, cutoff time.Time, limit int) ([]ThreadKey, error) {
	f.mu.Lock()
	f.cutoffs = append(f.cutoffs, cutoff)
	f.limits = append(f.limits, limit)
	listErr := f.listErr
	threads := append([]ThreadKey(nil), f.threads...)
	f.mu.Unlock()
	select {
	case f.ticks <- struct{}{}:
	default:
	}
	if listErr != nil {
		return nil, listErr
	}
	return threads, nil
}

func (f *fakeIdle) EraseIdle(_ context.Context, key ThreadKey, _ time.Time) (bool, int64, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.shared[key] {
		return false, 0, ErrThreadShared
	}
	if f.touched[key] {
		return false, 0, nil
	}
	f.erased = append(f.erased, key)
	return true, f.files, nil
}

func (f *fakeIdle) EraseStaleLeases(_ context.Context, cutoff time.Time) (int64, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.pruneCutoffs = append(f.pruneCutoffs, cutoff)
	return 0, f.pruneErr
}

// fakeBlobs records every thread whose blobs it was asked to remove.
type fakeBlobs struct {
	mu      sync.Mutex
	removed []string
	err     error
}

func (f *fakeBlobs) DeleteThreadBlobs(_ context.Context, orgID, threadID string) (int64, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.removed = append(f.removed, orgID+"/"+threadID)
	if f.err != nil {
		return 0, f.err
	}
	return 1, nil
}

var (
	idleThread   = ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_idle"}
	touchedAgain = ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_busy"}
)

// TestSweepErasesIdleThreadsAndTheirBlobsOnly: a thread that was used after the
// sweep listed it is kept, rows and blobs both.
func TestSweepErasesIdleThreadsAndTheirBlobsOnly(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle(idleThread, touchedAgain)
	rows.touched[touchedAgain] = true
	rows.files = 3
	blobs := &fakeBlobs{}
	now := time.Date(2026, 10, 31, 12, 0, 0, 0, time.UTC)

	erased, err := NewSweeper(rows, blobs, 30*24*time.Hour).sweep(context.Background(), now)
	if err != nil {
		t.Fatalf("sweep: %v", err)
	}
	if erased != 1 {
		t.Fatalf("erased = %d, want 1", erased)
	}
	if len(rows.erased) != 1 || rows.erased[0] != idleThread {
		t.Fatalf("rows erased = %v, want only the idle thread", rows.erased)
	}
	if len(blobs.removed) != 1 || blobs.removed[0] != "org_1/thr_idle" {
		t.Fatalf("blobs removed = %v, want only the idle thread", blobs.removed)
	}
	if want := now.Add(-30 * 24 * time.Hour); !rows.cutoffs[0].Equal(want) {
		t.Fatalf("cutoff = %s, want %s", rows.cutoffs[0], want)
	}
	if rows.limits[0] != sweepBatch {
		t.Fatalf("batch = %d, want %d", rows.limits[0], sweepBatch)
	}
}

// TestSweepKeepsTheErasureWhenBlobRemovalFails: the rows are already gone, so a
// blob failure is logged for an operator and does not fail the pass.
func TestSweepKeepsTheErasureWhenBlobRemovalFails(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle(idleThread)
	blobs := &fakeBlobs{err: errors.New("object store unavailable")}

	erased, err := NewSweeper(rows, blobs, time.Hour).sweep(context.Background(), time.Now())
	if err != nil {
		t.Fatalf("sweep: %v", err)
	}
	if erased != 1 || len(rows.erased) != 1 {
		t.Fatalf("erased = %d, rows erased = %v; want the rows erased despite the blob failure", erased, rows.erased)
	}
}

// TestSweepSkipsAThreadAnotherOwnerAlsoHolds: a shared thread id is kept with its
// blobs, and the pass goes on to the threads after it.
func TestSweepSkipsAThreadAnotherOwnerAlsoHolds(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle(touchedAgain, idleThread)
	rows.shared = map[ThreadKey]bool{touchedAgain: true}
	blobs := &fakeBlobs{}

	erased, err := NewSweeper(rows, blobs, time.Hour).sweep(context.Background(), time.Now())
	if err != nil || erased != 1 {
		t.Fatalf("erased = %d, err = %v; want the idle thread erased and the shared one skipped", erased, err)
	}
	if len(blobs.removed) != 1 || blobs.removed[0] != "org_1/thr_idle" {
		t.Fatalf("blobs removed = %v; a shared thread's blobs must not be touched", blobs.removed)
	}
}

func TestSweepReportsAListingFailureAndErasesNothing(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle(idleThread)
	rows.listErr = errors.New("connection reset")
	blobs := &fakeBlobs{}

	if _, err := NewSweeper(rows, blobs, time.Hour).sweep(context.Background(), time.Now()); !errors.Is(err, rows.listErr) {
		t.Fatalf("err = %v, want the listing failure", err)
	}
	if len(rows.erased) != 0 || len(blobs.removed) != 0 {
		t.Fatalf("erased %v and removed %v after a failed listing", rows.erased, blobs.removed)
	}
}

func TestSweepWithNothingIdleDoesNothing(t *testing.T) {
	t.Parallel()
	blobs := &fakeBlobs{}
	erased, err := NewSweeper(newFakeIdle(), blobs, time.Hour).sweep(context.Background(), time.Now())
	if err != nil || erased != 0 || len(blobs.removed) != 0 {
		t.Fatalf("erased = %d, err = %v, removed = %v; want nothing", erased, err, blobs.removed)
	}
}

// TestSweepPrunesStaleThreadLeasesOnEveryPass: the lease rows are pruned on every
// pass, including one with no idle workspace, at the same retention cutoff.
func TestSweepPrunesStaleThreadLeasesOnEveryPass(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle()
	now := time.Date(2026, 10, 31, 12, 0, 0, 0, time.UTC)

	erased, err := NewSweeper(rows, &fakeBlobs{}, 30*24*time.Hour).sweep(context.Background(), now)
	if err != nil || erased != 0 {
		t.Fatalf("erased = %d, err = %v; want nothing erased and no error", erased, err)
	}
	if len(rows.pruneCutoffs) != 1 || !rows.pruneCutoffs[0].Equal(now.Add(-30*24*time.Hour)) {
		t.Fatalf("lease prune cutoffs = %v, want one prune at the retention cutoff", rows.pruneCutoffs)
	}
}

// TestSweepKeepsTheErasuresWhenThePruneFails: the lease rows go after the
// workspaces, so a failed prune reports its error and leaves this pass's erasures
// done.
func TestSweepKeepsTheErasuresWhenThePruneFails(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle(idleThread)
	rows.pruneErr = errors.New("connection reset")
	blobs := &fakeBlobs{}

	erased, err := NewSweeper(rows, blobs, time.Hour).sweep(context.Background(), time.Now())
	if !errors.Is(err, rows.pruneErr) {
		t.Fatalf("err = %v, want the prune failure", err)
	}
	if erased != 1 || len(rows.erased) != 1 || len(blobs.removed) != 1 {
		t.Fatalf("erased = %d, rows = %v, blobs = %v; want the idle thread erased before the prune failed", erased, rows.erased, blobs.removed)
	}
}

// TestSweeperSweepsOnStartAndKeepsRunningAfterAFailedPass: a pass that fails must
// not stop the loop, and cancelling the context stops it.
func TestSweeperSweepsOnStartAndKeepsRunningAfterAFailedPass(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle()
	rows.listErr = errors.New("connection reset")
	sweeper := NewSweeper(rows, &fakeBlobs{}, time.Hour).WithTiming(10 * time.Millisecond)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan struct{})
	go func() {
		sweeper.Start(ctx)
		close(done)
	}()

	for i := 0; i < 2; i++ {
		select {
		case <-rows.ticks:
		case <-time.After(5 * time.Second):
			t.Fatalf("sweeper stopped after a failed pass (ran %d times)", i)
		}
	}
	cancel()
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("sweeper did not stop when its context was cancelled")
	}
}

func TestWithTimingKeepsTheDefaultForANonPositiveInterval(t *testing.T) {
	t.Parallel()
	sweeper := NewSweeper(newFakeIdle(), &fakeBlobs{}, time.Hour).WithTiming(0)
	if sweeper.interval != DefaultSweepInterval {
		t.Fatalf("interval = %s, want the default", sweeper.interval)
	}
}

// TestRetentionFromEnvDefaultsAndFailsLoudlyOnAMistake: an unset value takes the
// DEC-23 default, and a typo stops startup rather than quietly changing how long
// user files are kept.
func TestRetentionFromEnvDefaultsAndFailsLoudlyOnAMistake(t *testing.T) {
	t.Parallel()
	got, err := RetentionFromEnv(func(string) string { return "" })
	if err != nil || got != DefaultRetention {
		t.Fatalf("unset: retention = %s, err = %v; want the default", got, err)
	}

	env := map[string]string{"SANDBOX_MANAGER_THREAD_RETENTION_DAYS": "7"}
	got, err = RetentionFromEnv(func(key string) string { return env[key] })
	if err != nil || got != 7*24*time.Hour {
		t.Fatalf("7 days: retention = %s, err = %v", got, err)
	}

	for _, bad := range []string{"0", "-3", "soon", "1.5"} {
		env["SANDBOX_MANAGER_THREAD_RETENTION_DAYS"] = bad
		if _, err := RetentionFromEnv(func(key string) string { return env[key] }); err == nil {
			t.Fatalf("value %q was accepted", bad)
		}
	}
}

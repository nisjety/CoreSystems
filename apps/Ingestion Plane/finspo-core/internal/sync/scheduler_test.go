package sync

import (
	"context"
	"errors"
	"io"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/store"
)

type fakeEnabledLister struct {
	sources []store.Source
	err     error
}

func (f *fakeEnabledLister) ListEnabled(_ context.Context) ([]store.Source, error) {
	if f.err != nil {
		return nil, f.err
	}
	return f.sources, nil
}

type fakeSyncRunner struct {
	mu     sync.Mutex
	calls  []uuid.UUID
	result SyncResult
	err    error
}

func (f *fakeSyncRunner) SyncDrive(_ context.Context, id uuid.UUID) (SyncResult, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls = append(f.calls, id)
	if f.err != nil {
		return SyncResult{}, f.err
	}
	res := f.result
	res.SourceID = id
	return res, nil
}

func (f *fakeSyncRunner) callCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.calls)
}

func TestSchedulerRunsAllEnabledSourcesOnFirstTickThenStops(t *testing.T) {
	t.Parallel()

	src1 := store.Source{ID: uuid.New(), OrganizationID: "org-1", Enabled: true}
	src2 := store.Source{ID: uuid.New(), OrganizationID: "org-1", Enabled: true}
	lister := &fakeEnabledLister{sources: []store.Source{src1, src2}}
	runner := &fakeSyncRunner{}
	sched := NewScheduler(SchedulerConfig{
		Sources:  lister,
		Runner:   runner,
		Interval: 50 * time.Millisecond,
		Logger:   zerolog.New(io.Discard),
	})

	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		sched.Run(ctx)
		close(done)
	}()

	// Wait for the immediate sweep (both sources synced).
	deadline := time.After(time.Second)
	for {
		if runner.callCount() >= 2 {
			break
		}
		select {
		case <-deadline:
			t.Fatalf("scheduler never synced both sources (got %d)", runner.callCount())
		case <-time.After(5 * time.Millisecond):
		}
	}

	cancel()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("scheduler did not stop within 1s of cancel")
	}
}

func TestSchedulerLogsAndContinuesOnSyncError(t *testing.T) {
	t.Parallel()

	src := store.Source{ID: uuid.New(), OrganizationID: "org-1", Enabled: true}
	lister := &fakeEnabledLister{sources: []store.Source{src, src}} // intentionally duplicated to count two calls
	runner := &fakeSyncRunner{err: errors.New("boom")}
	sched := NewScheduler(SchedulerConfig{
		Sources:  lister,
		Runner:   runner,
		Interval: time.Hour, // only the first immediate sweep should run
		Logger:   zerolog.New(io.Discard),
	})

	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		sched.Run(ctx)
		close(done)
	}()

	deadline := time.After(time.Second)
	for runner.callCount() < 2 {
		select {
		case <-deadline:
			t.Fatalf("expected 2 sync attempts, got %d", runner.callCount())
		case <-time.After(5 * time.Millisecond):
		}
	}

	cancel()
	<-done
}

func TestSchedulerSkipsTickWhenNoEnabledSources(t *testing.T) {
	t.Parallel()

	lister := &fakeEnabledLister{sources: nil}
	runner := &fakeSyncRunner{}
	sched := NewScheduler(SchedulerConfig{
		Sources:  lister,
		Runner:   runner,
		Interval: time.Hour,
		Logger:   zerolog.New(io.Discard),
	})

	ctx, cancel := context.WithCancel(context.Background())
	go sched.Run(ctx)
	time.Sleep(50 * time.Millisecond)
	cancel()

	if got := runner.callCount(); got != 0 {
		t.Fatalf("callCount = %d, want 0", got)
	}
}

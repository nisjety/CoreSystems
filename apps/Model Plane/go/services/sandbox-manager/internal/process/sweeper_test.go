package process

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"
)

// sweepRecorder is a staleSweeper that records each call and signals a
// channel, so the loop can be observed without a database or a sleep.
type sweepRecorder struct {
	mu     sync.Mutex
	calls  []time.Duration
	result int64
	err    error
	swept  chan struct{}
}

func newSweepRecorder() *sweepRecorder {
	return &sweepRecorder{swept: make(chan struct{}, 16)}
}

func (r *sweepRecorder) SweepStale(_ context.Context, staleAfter time.Duration) (int64, error) {
	r.mu.Lock()
	r.calls = append(r.calls, staleAfter)
	r.mu.Unlock()
	select {
	case r.swept <- struct{}{}:
	default:
	}
	return r.result, r.err
}

func (r *sweepRecorder) callCount() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return len(r.calls)
}

func (r *sweepRecorder) firstStaleAfter(t *testing.T) time.Duration {
	t.Helper()
	r.mu.Lock()
	defer r.mu.Unlock()
	if len(r.calls) == 0 {
		t.Fatal("sweeper never ran")
	}
	return r.calls[0]
}

// TestSweeperSweepsImmediatelyOnStart is the restart property: a replica
// that has just booted must re-scan at once rather than leave rows claiming
// to be RUNNING for a whole interval.
func TestSweeperSweepsImmediatelyOnStart(t *testing.T) {
	t.Parallel()
	recorder := newSweepRecorder()
	sweeper := NewSweeper(recorder).WithTiming(time.Hour, 90*time.Second)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan struct{})
	go func() {
		sweeper.Start(ctx)
		close(done)
	}()

	select {
	case <-recorder.swept:
	case <-time.After(5 * time.Second):
		t.Fatal("sweeper did not run before its first tick")
	}
	if got := recorder.firstStaleAfter(t); got != 90*time.Second {
		t.Fatalf("stale window = %s, want 90s", got)
	}

	cancel()
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("sweeper did not stop when its context was cancelled")
	}
}

// TestSweeperKeepsRunningAfterAFailedSweep: a registry that cannot reach
// Postgres for a moment must not take the loop (or the service) down.
func TestSweeperKeepsRunningAfterAFailedSweep(t *testing.T) {
	t.Parallel()
	recorder := newSweepRecorder()
	recorder.err = errors.New("connection reset")
	sweeper := NewSweeper(recorder).WithTiming(10*time.Millisecond, time.Minute)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan struct{})
	go func() {
		sweeper.Start(ctx)
		close(done)
	}()

	for i := 0; i < 2; i++ {
		select {
		case <-recorder.swept:
		case <-time.After(5 * time.Second):
			t.Fatalf("sweeper stopped after a failure (ran %d times)", recorder.callCount())
		}
	}

	cancel()
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("sweeper did not stop when its context was cancelled")
	}
}

func TestWithTimingKeepsDefaultsForNonPositiveValues(t *testing.T) {
	t.Parallel()
	sweeper := NewSweeper(newSweepRecorder()).WithTiming(0, -time.Second)
	if sweeper.interval != DefaultSweepInterval {
		t.Fatalf("interval = %s, want the default", sweeper.interval)
	}
	if sweeper.staleAfter != DefaultStaleAfter {
		t.Fatalf("staleAfter = %s, want the default", sweeper.staleAfter)
	}
}

func TestSweeperEnabledIsOptOutNotOptIn(t *testing.T) {
	t.Parallel()
	for _, value := range []string{"", "true", "1", "yes", "anything"} {
		if !SweeperEnabled(value) {
			t.Fatalf("value %q should leave the sweeper enabled", value)
		}
	}
	for _, value := range []string{"false", " false "} {
		if SweeperEnabled(value) {
			t.Fatalf("value %q should disable the sweeper", value)
		}
	}
}

// TestTimingFromEnvFallsBackRatherThanFailing: a typo in an optional tuning
// knob must not stop the service, because the default is always a safe
// answer.
func TestTimingFromEnvFallsBackRatherThanFailing(t *testing.T) {
	t.Parallel()
	env := map[string]string{
		"PROCESS_SWEEP_INTERVAL_SECS":  "5",
		"PROCESS_HEARTBEAT_STALE_SECS": "45",
	}
	interval, staleAfter := TimingFromEnv(func(key string) string { return env[key] })
	if interval != 5*time.Second || staleAfter != 45*time.Second {
		t.Fatalf("interval = %s, staleAfter = %s", interval, staleAfter)
	}

	for _, bad := range []string{"", "  ", "0", "-30", "soon"} {
		env["PROCESS_SWEEP_INTERVAL_SECS"] = bad
		env["PROCESS_HEARTBEAT_STALE_SECS"] = bad
		interval, staleAfter = TimingFromEnv(func(key string) string { return env[key] })
		if interval != DefaultSweepInterval || staleAfter != DefaultStaleAfter {
			t.Fatalf("value %q did not fall back: interval = %s, staleAfter = %s", bad, interval, staleAfter)
		}
	}
}

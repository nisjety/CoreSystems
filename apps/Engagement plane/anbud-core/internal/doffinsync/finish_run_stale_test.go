package doffinsync

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// committedThenTransient is a run ledger whose first FinishRun commits the outcome
// and then returns a transient error, as a lost reply does. Every later call finds
// the run no longer running and returns ErrStaleWrite.
type committedThenTransient struct {
	*fakeStore
	calls int
}

func (c *committedThenTransient) FinishRun(ctx context.Context, runID int64, out RunOutcome) error {
	c.calls++
	if c.calls == 1 {
		if err := c.fakeStore.FinishRun(ctx, runID, out); err != nil {
			return err
		}
		return errFinish
	}
	return ErrStaleWrite
}

// alwaysStale is a run ledger whose run is no longer running on every call.
type alwaysStale struct {
	*fakeStore
	calls int
}

func (a *alwaysStale) FinishRun(context.Context, int64, RunOutcome) error {
	a.calls++
	return ErrStaleWrite
}

// oneDoneDayEngine runs one fully held day, so the run ends done, on the given run ledger.
func oneDoneDayEngine(t *testing.T, h *harness, runs RunLedger) *Engine {
	t.Helper()
	all := ids(1, 1)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(1, 1, all), nil
	}
	h.store.resolve = held(all)
	deps := h.deps()
	deps.Runs = runs
	e, err := New(deps, Options{Mode: ModeSync, Now: func() time.Time { return testNow }})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return e
}

// TestStaleWriteAfterAnEarlierFailedTryIsSuccess (A3, D-B9): the first FinishRun
// committed and then failed, so the second sees the run no longer running. The
// record exists, so the run returns no error and keeps the reason it recorded.
func TestStaleWriteAfterAnEarlierFailedTryIsSuccess(t *testing.T) {
	t.Parallel()

	h := newHarness()
	runs := &committedThenTransient{fakeStore: h.store}
	e := oneDoneDayEngine(t, h, runs)
	d := day(2026, 10, 7)

	sum, err := e.Run(context.Background(), Window{From: d, To: d})

	switch {
	case err != nil:
		t.Errorf("Run error = %v, want nil: the stale write follows a try that committed", err)
	case runs.calls != 2:
		t.Errorf("FinishRun calls = %d, want 2", runs.calls)
	case sum.Reason != ReasonDone:
		t.Errorf("summary reason = %s, want done", sum.Reason)
	case len(h.store.runOutcomes) != 1 || h.store.runOutcomes[0].Reason != ReasonDone:
		t.Errorf("recorded outcomes = %+v, want one done outcome", h.store.runOutcomes)
	}
}

// TestStaleWriteOnTheFirstTryIsStillAnError (A3): when the very first FinishRun
// finds the run no longer running, nothing was recorded by this process. That is
// an error, and a stale write is not retried, because retrying cannot succeed.
func TestStaleWriteOnTheFirstTryIsStillAnError(t *testing.T) {
	t.Parallel()

	h := newHarness()
	runs := &alwaysStale{fakeStore: h.store}
	e := oneDoneDayEngine(t, h, runs)
	d := day(2026, 10, 7)

	_, err := e.Run(context.Background(), Window{From: d, To: d})

	switch {
	case !errors.Is(err, ErrStaleWrite):
		t.Errorf("Run error = %v, want ErrStaleWrite on the first try", err)
	case runs.calls != 1:
		t.Errorf("FinishRun calls = %d, want 1: a stale first try is not retried", runs.calls)
	}
}

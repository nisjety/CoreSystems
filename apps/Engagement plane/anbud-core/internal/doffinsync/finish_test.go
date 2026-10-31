package doffinsync

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// errFinish is the failure the flaky ledger returns from FinishRun.
var errFinish = errors.New("run ledger unavailable")

// flakyRuns fails the first fails calls to FinishRun and passes the rest through
// to the fake store. Every call is counted.
type flakyRuns struct {
	*fakeStore
	fails int
	calls int
}

func (f *flakyRuns) FinishRun(ctx context.Context, runID int64, out RunOutcome) error {
	f.calls++
	if f.calls <= f.fails {
		return errFinish
	}
	return f.fakeStore.FinishRun(ctx, runID, out)
}

// finishRunEngine runs one fully held day, so the run is done, with a ledger
// that fails the first fails FinishRun calls.
func finishRunEngine(t *testing.T, h *harness, fails int) (*Engine, *flakyRuns) {
	t.Helper()
	all := ids(1, 1)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(1, 1, all), nil
	}
	h.store.resolve = held(all)
	runs := &flakyRuns{fakeStore: h.store, fails: fails}
	deps := h.deps()
	deps.Runs = runs
	e, err := New(deps, Options{Mode: ModeSync, Now: func() time.Time { return testNow }})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return e, runs
}

// TestFinishRunRetriesUntilItSucceeds (E7, D-B9): a run record that fails twice
// and then succeeds is written. The run returns no error and the right reason.
func TestFinishRunRetriesUntilItSucceeds(t *testing.T) {
	t.Parallel()

	h := newHarness()
	e, runs := finishRunEngine(t, h, 2)
	d := day(2026, 10, 7)
	sum, err := e.Run(context.Background(), Window{From: d, To: d})

	switch {
	case err != nil:
		t.Errorf("Run error = %v, want nil after the third try succeeds", err)
	case runs.calls != 3:
		t.Errorf("FinishRun calls = %d, want 3", runs.calls)
	case sum.Reason != ReasonDone:
		t.Errorf("run reason = %s, want done", sum.Reason)
	case len(h.store.runOutcomes) != 1 || h.store.runOutcomes[0].Reason != ReasonDone:
		t.Errorf("run outcomes = %+v, want one done outcome", h.store.runOutcomes)
	}
}

// TestFinishRunGivesUpAfterThreeTries (E7, D-B9): a run record that fails three
// times is an error, and the summary still carries the day outcomes.
func TestFinishRunGivesUpAfterThreeTries(t *testing.T) {
	t.Parallel()

	h := newHarness()
	e, runs := finishRunEngine(t, h, 3)
	d := day(2026, 10, 7)
	sum, err := e.Run(context.Background(), Window{From: d, To: d})

	switch {
	case !errors.Is(err, errFinish):
		t.Errorf("Run error = %v, want the ledger error after three tries", err)
	case runs.calls != 3:
		t.Errorf("FinishRun calls = %d, want exactly 3", runs.calls)
	case len(sum.Days) != 1 || sum.Days[0].State != StateDone:
		t.Errorf("summary days = %+v, want the one done day", sum.Days)
	case sum.Reason != ReasonDone:
		t.Errorf("summary reason = %s, want done", sum.Reason)
	}
}

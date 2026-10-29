package doffinsync

import (
	"context"
	"testing"
	"time"
)

// cancelDayStates wraps the fake store. DayStates cancels the run and then
// fails, which is what a read does when its context ends mid-query.
type cancelDayStates struct {
	*fakeStore
	cancel context.CancelFunc
}

func (c cancelDayStates) DayStates(context.Context, time.Time, time.Time) (map[string]DayState, error) {
	c.cancel()
	return nil, errWriteCut
}

// TestCancelDuringDayStatesIsCancelledNotLedgerFailed: a run context that ends
// while the day states are read is a cancelled run, not a ledger failure. Run
// returns nil, the run records cancelled, and no Doffin call is made.
func TestCancelDuringDayStatesIsCancelledNotLedgerFailed(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d := day(2026, 10, 7)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	deps := h.deps()
	deps.Days = cancelDayStates{fakeStore: h.store, cancel: cancel}
	e, err := New(deps, Options{Mode: ModeSync, Now: func() time.Time { return testNow }})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sum, err := e.Run(ctx, Window{From: d, To: d})
	if err != nil {
		t.Errorf("Run error = %v, want nil: a read cut off by the run's own cancellation is not a ledger failure", err)
	}

	switch {
	case sum.Reason != ReasonCancelled:
		t.Errorf("run reason = %s, want cancelled", sum.Reason)
	case len(h.store.runOutcomes) != 1:
		t.Fatalf("run outcomes = %d, want 1", len(h.store.runOutcomes))
	}
	out := h.store.runOutcomes[0]
	switch {
	case out.Reason != ReasonCancelled:
		t.Errorf("recorded outcome = %s, want cancelled", out.Reason)
	case out.LastError != msgCancelled:
		t.Errorf("recorded last error = %q, want %q", out.LastError, msgCancelled)
	case len(h.api.searches) != 0:
		t.Errorf("searches = %d, want none after cancellation", len(h.api.searches))
	}
}

// TestDayStatesFailureWithOpenContextIsStillLedgerFailed: a read error while the
// run is still open stays a ledger failure, so the run is not hidden as cancelled.
func TestDayStatesFailureWithOpenContextIsStillLedgerFailed(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d := day(2026, 10, 7)
	h.store.dayStatesErr = errWriteCut

	sum, err := h.engine(t, ModeSync).Run(context.Background(), Window{From: d, To: d})
	if err == nil {
		t.Fatal("Run error = nil, want the ledger failure")
	}
	if sum.Reason != ReasonFailed {
		t.Errorf("run reason = %s, want failed", sum.Reason)
	}
	if len(h.store.runOutcomes) != 1 || h.store.runOutcomes[0].Reason != ReasonFailed {
		t.Errorf("run outcomes = %+v, want one failed outcome", h.store.runOutcomes)
	}
}

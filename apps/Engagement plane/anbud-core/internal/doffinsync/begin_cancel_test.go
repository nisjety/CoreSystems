package doffinsync

import (
	"context"
	"testing"
	"time"
)

// cancelBegin wraps the fake store. BeginDay cancels the run and then fails, which
// is what a ledger write does when its context ends during the write.
type cancelBegin struct {
	*fakeStore
	cancel context.CancelFunc
}

func (c cancelBegin) BeginDay(context.Context, time.Time, int64) error {
	c.cancel()
	return errWriteCut
}

// TestCancelDuringBeginDayIsCancelledNotLedgerFailed: a run context that ends
// during BeginDay cancels the run before any Doffin call. The ledger is not at
// fault, so Run returns nil and the run records cancelled, not a ledger failure.
func TestCancelDuringBeginDayIsCancelledNotLedgerFailed(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d := day(2026, 10, 7)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	deps := h.deps()
	deps.Days = cancelBegin{fakeStore: h.store, cancel: cancel}
	e, err := New(deps, Options{Mode: ModeSync, Now: func() time.Time { return testNow }})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	sum, err := e.Run(ctx, Window{From: d, To: d})
	if err != nil {
		t.Errorf("Run error = %v, want nil: a cancelled begin is not a ledger failure", err)
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

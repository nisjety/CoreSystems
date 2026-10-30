package doffinsync

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// errStoreNotice is the notice store failure of the join test.
var errStoreNotice = errors.New("notice store unavailable")

// errDayAtFinish is the day ledger failure that arrives after the store failure.
var errDayAtFinish = errors.New("day ledger unavailable at finish")

// TestStoreFailureAndFinishDayFailureAreBothReachable (A2): when a day has already
// halted on a store error and its FinishDay also fails, the run error keeps both.
// errors.Is finds each cause, so neither is lost from the audit.
func TestStoreFailureAndFinishDayFailureAreBothReachable(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d := day(2026, 10, 7)
	all := ids(1, 1)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(1, 1, all), nil
	}
	h.store.storeErr = errStoreNotice
	h.store.finishDayErr = errDayAtFinish

	e := h.engine(t, ModeSync)
	sum, err := e.Run(context.Background(), Window{From: d, To: d})

	switch {
	case err == nil:
		t.Fatal("Run error = nil, want both the store failure and the FinishDay failure")
	case !errors.Is(err, errStoreNotice):
		t.Errorf("Run error = %v, want errors.Is to find the store failure", err)
	case !errors.Is(err, errDayAtFinish):
		t.Errorf("Run error = %v, want errors.Is to find the FinishDay failure", err)
	case sum.Reason != ReasonFailed:
		t.Errorf("run reason = %s, want failed", sum.Reason)
	}
}

// TestFinishDayFailureAloneStillFailsTheRun: with no earlier halt, a FinishDay
// failure is the run error, as before.
func TestFinishDayFailureAloneStillFailsTheRun(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d := day(2026, 10, 7)
	all := ids(1, 1)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(1, 1, all), nil
	}
	h.store.resolve = held(all)
	h.store.finishDayErr = errDayAtFinish

	e := h.engine(t, ModeSync, func(o *Options) { o.Now = func() time.Time { return testNow } })
	sum, err := e.Run(context.Background(), Window{From: d, To: d})

	switch {
	case !errors.Is(err, errDayAtFinish):
		t.Errorf("Run error = %v, want the FinishDay failure", err)
	case sum.Reason != ReasonFailed:
		t.Errorf("run reason = %s, want failed", sum.Reason)
	}
}

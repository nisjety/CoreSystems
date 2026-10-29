package doffinsync

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// errWriteCut is what the cancelling store returns from the write it interrupts.
var errWriteCut = errors.New("write cut off")

// cancelOn wraps the fake store. The write named by op cancels the run and then
// fails, which is what a database call does when its context ends mid-write.
type cancelOn struct {
	*fakeStore
	op     string // "page", "resolve", "store" or "attempt"
	cancel context.CancelFunc
}

// cut reports whether op is the write to interrupt, and cancels the run if so.
func (c cancelOn) cut(op string) bool {
	if op != c.op {
		return false
	}
	c.cancel()
	return true
}

func (c cancelOn) PutSearchPage(ctx context.Context, runID int64, p SearchPage) error {
	if c.cut("page") {
		return errWriteCut
	}
	return c.fakeStore.PutSearchPage(ctx, runID, p)
}

func (c cancelOn) Resolve(ctx context.Context, ids []string) (map[string]Resolution, error) {
	if c.cut("resolve") {
		return nil, errWriteCut
	}
	return c.fakeStore.Resolve(ctx, ids)
}

func (c cancelOn) StoreNotice(ctx context.Context, runID int64, n doffin.Notice) (bool, error) {
	if c.cut("store") {
		return false, errWriteCut
	}
	return c.fakeStore.StoreNotice(ctx, runID, n)
}

func (c cancelOn) RecordAttempt(ctx context.Context, runID int64, a Attempt) error {
	if c.cut("attempt") {
		return errWriteCut
	}
	return c.fakeStore.RecordAttempt(ctx, runID, a)
}

// TestCancelDuringStoreWriteIsCancelledNotStoreFailed (E1, D-B8): a run context
// that ends during a store write interrupts the day and cancels the run. It is
// not a store fault. The id is downloaded again on the next run.
func TestCancelDuringStoreWriteIsCancelledNotStoreFailed(t *testing.T) {
	t.Parallel()

	serverError := func(context.Context, string) (doffin.Notice, error) {
		return doffin.Notice{}, &doffin.HTTPError{Operation: "download", Status: 500, Message: "busy"}
	}
	cases := []struct {
		name          string
		op            string
		download      func(context.Context, string) (doffin.Notice, error)
		wantDownloads int
	}{
		{"search page", "page", nil, 0},
		{"resolve", "resolve", nil, 0},
		{"store notice", "store", nil, 1},
		{"record attempt", "attempt", serverError, 1},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			h := newHarness()
			d := day(2026, 10, 7)
			all := ids(1, 2)
			h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
				return searchResp(2, 2, all), nil
			}
			h.api.download = tc.download

			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			cut := cancelOn{fakeStore: h.store, op: tc.op, cancel: cancel}
			deps := h.deps()
			deps.Pages, deps.Notices = cut, cut
			e, err := New(deps, Options{Mode: ModeSync, Now: func() time.Time { return testNow }})
			if err != nil {
				t.Fatalf("New: %v", err)
			}
			sum, err := e.Run(ctx, Window{From: d, To: d})
			if err != nil {
				t.Errorf("Run error = %v, want nil: a cancelled write is not a ledger failure", err)
			}

			rec := h.finishedDay(t, d)
			switch {
			case rec.State != StateInterrupted || rec.ErrorCode != CodeCancelled:
				t.Errorf("day = %s/%s, want interrupted/cancelled", rec.State, rec.ErrorCode)
			case sum.Reason != ReasonCancelled:
				t.Errorf("run reason = %s, want cancelled", sum.Reason)
			case len(h.api.downloads) != tc.wantDownloads:
				t.Errorf("downloads = %d, want %d", len(h.api.downloads), tc.wantDownloads)
			case len(h.store.attempts) != 0:
				t.Errorf("attempts = %+v, want none: a cut write leaves no row", h.store.attempts)
			}
			for _, r := range h.store.finished {
				if r.ErrorCode == CodeStoreFailed {
					t.Errorf("a day carries store_failed, want cancelled: %+v", r)
				}
			}
		})
	}
}

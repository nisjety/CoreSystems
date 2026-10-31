package doffinsync

import (
	"context"
	"fmt"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// TestCancelledBackOffKeepsTheRetryInstant (A5): a download or search cut short
// by the run's own cancellation while it waited out a rate-limit back-off ends the
// day as interrupted and the run as cancelled. The run also keeps the instant
// before which Doffin must not be called: the engine clock plus RetryAfter, in UTC.
func TestCancelledBackOffKeepsTheRetryInstant(t *testing.T) {
	t.Parallel()

	const retryAfter = 25 * time.Second
	rateLimited := func(op string) error {
		return &doffin.RateLimitedError{Operation: op, RetryAfter: retryAfter}
	}
	cases := []struct {
		name   string
		search func(cancel context.CancelFunc) func(int, doffin.SearchParams) (doffin.SearchResponse, error)
		down   func(cancel context.CancelFunc) func(context.Context, string) (doffin.Notice, error)
	}{
		{
			name: "download",
			search: func(context.CancelFunc) func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
				all := ids(1, 1)
				return func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
					return searchResp(1, 1, all), nil
				}
			},
			down: func(cancel context.CancelFunc) func(context.Context, string) (doffin.Notice, error) {
				return func(context.Context, string) (doffin.Notice, error) {
					cancel()
					return doffin.Notice{}, fmt.Errorf("%w: %w", context.Canceled, rateLimited("download"))
				}
			},
		},
		{
			name: "search",
			search: func(cancel context.CancelFunc) func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
				return func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
					cancel()
					return doffin.SearchResponse{}, fmt.Errorf("%w: %w", context.Canceled, rateLimited("search"))
				}
			},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			h := newHarness()
			d := day(2026, 10, 7)
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			h.api.search = tc.search(cancel)
			if tc.down != nil {
				h.api.download = tc.down(cancel)
			}

			sum, err := h.engine(t, ModeSync).Run(ctx, Window{From: d, To: d})
			if err != nil {
				t.Errorf("Run error = %v, want nil: a cancelled back-off is not a ledger failure", err)
			}

			rec := h.finishedDay(t, d)
			wantUntil := testNow.Add(retryAfter).UTC()
			switch {
			case rec.State != StateInterrupted || rec.ErrorCode != CodeCancelled:
				t.Errorf("day = %s/%s, want interrupted/cancelled", rec.State, rec.ErrorCode)
			case sum.Reason != ReasonCancelled:
				t.Errorf("run reason = %s, want cancelled", sum.Reason)
			case len(h.store.runOutcomes) != 1:
				t.Fatalf("run outcomes = %d, want 1", len(h.store.runOutcomes))
			case h.store.runOutcomes[0].Reason != ReasonCancelled:
				t.Errorf("recorded reason = %s, want cancelled", h.store.runOutcomes[0].Reason)
			case h.store.runOutcomes[0].RetryAfterUntil == nil:
				t.Errorf("recorded RetryAfterUntil = nil, want %s", wantUntil)
			case !h.store.runOutcomes[0].RetryAfterUntil.Equal(wantUntil):
				t.Errorf("recorded RetryAfterUntil = %s, want %s", h.store.runOutcomes[0].RetryAfterUntil, wantUntil)
			case sum.RetryAfterUntil == nil || !sum.RetryAfterUntil.Equal(wantUntil):
				t.Errorf("summary RetryAfterUntil = %v, want %s", sum.RetryAfterUntil, wantUntil)
			}
		})
	}
}

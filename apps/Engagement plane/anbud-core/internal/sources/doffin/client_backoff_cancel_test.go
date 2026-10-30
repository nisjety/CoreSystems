package doffin

import (
	"context"
	"errors"
	"net/http"
	"sync/atomic"
	"testing"
	"time"
)

// A context that ends during a 429 back-off must keep the retry gate. The error
// wraps the context error and a *RateLimitedError whose RetryAfter is the gate
// still closed when the back-off was cut short, so a caller can reschedule for
// that same instant.
func TestCancelDuringBackOffKeepsTheRemainingGate(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name     string
		elapsed  time.Duration // how far the fake clock moved before the cancel
		wantLeft time.Duration
	}{
		{"cancelled before the back-off starts", 0, 40 * time.Second},
		{"cancelled 15s into the back-off", 15 * time.Second, 25 * time.Second},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			var calls atomic.Int32
			c, _, clock := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				calls.Add(1)
				_, _ = w.Write([]byte(`{"hits":[]}`))
			}))
			c.closeGate(opSearch, 40*time.Second)

			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			c.sleep = func(ctx context.Context, _ time.Duration) error {
				clock.advance(tc.elapsed)
				cancel()
				return ctx.Err()
			}

			_, err := c.Search(ctx, SearchParams{})

			var rateErr *RateLimitedError
			switch {
			case !errors.Is(err, context.Canceled):
				t.Fatalf("error = %v, want it to wrap context.Canceled", err)
			case !errors.As(err, &rateErr):
				t.Fatalf("error = %v, want it to carry a *RateLimitedError", err)
			case rateErr.Operation != string(opSearch):
				t.Errorf("operation = %q, want %q", rateErr.Operation, opSearch)
			case rateErr.RetryAfter != tc.wantLeft:
				t.Errorf("RetryAfter = %v, want the remaining gate %v", rateErr.RetryAfter, tc.wantLeft)
			case calls.Load() != 0:
				t.Errorf("the cancelled back-off reached the API (calls = %d)", calls.Load())
			}
		})
	}
}

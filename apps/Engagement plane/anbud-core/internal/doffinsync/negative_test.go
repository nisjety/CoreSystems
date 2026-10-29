package doffinsync

import (
	"testing"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// TestNegativeCountIsDecodeError (E5, D-B1): a negative hit count is a decode
// error. The day fails with decode_error and no count is written. The response
// body is still kept as evidence, and nothing is downloaded from it.
func TestNegativeCountIsDecodeError(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name              string
		total, accessible int64
	}{
		{"negative total", -1, 1},
		{"negative accessible", 1, -1},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			h := newHarness()
			h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
				return searchResp(tc.total, tc.accessible, ids(1, 1)), nil
			}
			d := day(2026, 10, 7)
			h.run(t, h.engine(t, ModeSync), d, d)

			rec := h.finishedDay(t, d)
			switch {
			case rec.State != StateFailed || rec.ErrorCode != CodeDecodeError:
				t.Errorf("day = %s/%s, want failed/decode_error", rec.State, rec.ErrorCode)
			case rec.Total != nil || rec.Accessible != nil:
				t.Errorf("counts written = %v %v, want none from a negative response", rec.Total, rec.Accessible)
			case len(h.api.downloads) != 0 || len(h.store.notices) != 0 || len(h.store.attempts) != 0:
				t.Errorf("downloads %d notices %d attempts %d, want none", len(h.api.downloads), len(h.store.notices), len(h.store.attempts))
			case len(h.store.pages) != 1:
				t.Errorf("search pages kept = %d, want the 2xx body kept as evidence", len(h.store.pages))
			}
		})
	}
}

// TestNegativeCountsCountTowardConsecutiveLimit (E5, D-B1): decode errors from
// negative counts count toward the five-in-a-row limit, so six failing days stop
// the run after the fifth.
func TestNegativeCountsCountTowardConsecutiveLimit(t *testing.T) {
	t.Parallel()

	h := newHarness()
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(-1, 1, nil), nil
	}
	first := day(2026, 10, 3)
	last := first.AddDate(0, 0, 5) // six days: 2026-10-03 to 2026-10-08
	sum := h.run(t, h.engine(t, ModeSync), first, last)

	switch {
	case sum.Reason != ReasonFailed:
		t.Errorf("run reason = %s, want failed after five decode errors", sum.Reason)
	case len(sum.Days) != 5:
		t.Errorf("day outcomes = %d, want 5: the run must stop at the fifth", len(sum.Days))
	}
}

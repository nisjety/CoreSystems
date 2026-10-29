package doffinsync

import (
	"testing"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// TestFinishDayReceivesTheRunID (A6): every DayRecord the engine writes carries the
// id of the run that began the day, so the ledger can keep the write only for that
// run. The fake BeginRun returns id 7.
func TestFinishDayReceivesTheRunID(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name string
		mode Mode
	}{
		{"sync", ModeSync},
		{"census", ModeCensus},
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
			h.store.resolve = held(all)

			sum := h.run(t, h.engine(t, tc.mode), d, d)

			const wantRunID = 7
			if sum.RunID != wantRunID {
				t.Fatalf("summary run id = %d, want %d", sum.RunID, wantRunID)
			}
			if rec := h.finishedDay(t, d); rec.RunID != wantRunID {
				t.Errorf("FinishDay run id = %d, want %d", rec.RunID, wantRunID)
			}
		})
	}
}

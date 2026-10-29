package main

import (
	"bytes"
	"encoding/json"
	"reflect"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// R4: the day line carries "rejected", the count of terminal rejections, between
// "not_found" and "unresolved". The field is always present: a day with none says 0,
// so a reader never has to tell an absent key from a zero. The line holds counts,
// states and the day only, so the key set is pinned whole (T40).
func TestDayLineCarriesTheRejectedCount(t *testing.T) {
	t.Parallel()

	day := time.Date(2026, time.October, 7, 0, 0, 0, 0, time.UTC)
	cases := []struct {
		name string
		in   doffinsync.DayOutcome
		want map[string]any
	}{
		{
			name: "a day with terminal rejections",
			in: doffinsync.DayOutcome{
				Day: day, State: doffinsync.StatePartial, ErrorCode: doffinsync.CodeItemRejected,
				Total: 9, Accessible: 9, Held: 2, Stored: 3, NotFound: 1, Rejected: 2, Unresolved: 1,
				Searches: 4,
			},
			want: map[string]any{
				"day": "2026-10-07", "state": "partial", "error_code": "item_rejected",
				"total": 9.0, "accessible": 9.0, "held": 2.0, "stored": 3.0,
				"not_found": 1.0, "rejected": 2.0, "unresolved": 1.0, "searches": 4.0,
			},
		},
		{
			name: "a done day with none",
			in: doffinsync.DayOutcome{
				Day: day, State: doffinsync.StateDone, Total: 4, Accessible: 4, Held: 4, Searches: 2,
			},
			want: map[string]any{
				"day": "2026-10-07", "state": "done",
				"total": 4.0, "accessible": 4.0, "held": 4.0, "stored": 0.0,
				"not_found": 0.0, "rejected": 0.0, "unresolved": 0.0, "searches": 2.0,
			},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			var buf bytes.Buffer
			rep := newReport(&buf)
			rep.emit(newDayLine(tc.in))
			if rep.err != nil {
				t.Fatalf("emit: %v", rep.err)
			}
			var got map[string]any
			if err := json.Unmarshal(buf.Bytes(), &got); err != nil {
				t.Fatalf("the day line is not one JSON object: %v", err)
			}
			if !reflect.DeepEqual(got, tc.want) {
				t.Errorf("day line = %v, want %v", got, tc.want)
			}
		})
	}
}

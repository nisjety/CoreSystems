package doffinsync

import (
	"testing"
	"time"
)

// T12
func TestWindowIsClosedUTCDays(t *testing.T) {
	t.Parallel()

	utc := time.Date(2026, time.October, 9, 12, 0, 0, 0, time.UTC)
	cest := time.FixedZone("CEST", 2*60*60)
	// 00:30 on the 9th in Oslo is still the 8th in UTC, so UTC yesterday is the 7th.
	oslo := time.Date(2026, time.October, 9, 0, 30, 0, 0, cest)

	tests := []struct {
		name string
		w    Window
		now  time.Time
		ok   bool
	}{
		{"yesterday is closed", Window{day(2026, 10, 7), day(2026, 10, 8)}, utc, true},
		{"today is open", Window{day(2026, 10, 9), day(2026, 10, 9)}, utc, false},
		{"tomorrow is open", Window{day(2026, 10, 8), day(2026, 10, 10)}, utc, false},
		{"before 2017-01-02 is refused", Window{day(2017, 1, 1), day(2017, 1, 3)}, utc, false},
		{"2017-01-02 is the first day", Window{day(2017, 1, 2), day(2017, 1, 2)}, utc, true},
		{"from after to is refused", Window{day(2026, 10, 8), day(2026, 10, 7)}, utc, false},
		{"a local date ahead of UTC does not open a day", Window{day(2026, 10, 8), day(2026, 10, 8)}, oslo, false},
		{"the UTC yesterday is still closed in Oslo", Window{day(2026, 10, 7), day(2026, 10, 7)}, oslo, true},
		{"a bound with a time of day is refused", Window{time.Date(2026, 10, 7, 10, 0, 0, 0, time.UTC), day(2026, 10, 7)}, utc, false},
		{"a bound in another zone is refused", Window{time.Date(2026, 10, 7, 0, 0, 0, 0, cest), day(2026, 10, 7)}, utc, false},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := tc.w.Validate(tc.now)
			if (err == nil) != tc.ok {
				t.Fatalf("Validate(%v..%v) error = %v, want ok=%v", tc.w.From, tc.w.To, err, tc.ok)
			}
		})
	}

	t.Run("days are inclusive UTC midnights across a month end", func(t *testing.T) {
		got := Window{From: day(2026, 1, 30), To: day(2026, 2, 2)}.Days()
		want := []time.Time{day(2026, 1, 30), day(2026, 1, 31), day(2026, 2, 1), day(2026, 2, 2)}
		if len(got) != len(want) {
			t.Fatalf("Days() returned %d days, want %d", len(got), len(want))
		}
		for i := range want {
			if !got[i].Equal(want[i]) || got[i].Location() != time.UTC {
				t.Errorf("Days()[%d] = %v (%s), want %v in UTC", i, got[i], got[i].Location(), want[i])
			}
		}
	})

	t.Run("an inverted window has no days", func(t *testing.T) {
		if got := (Window{From: day(2026, 10, 8), To: day(2026, 10, 7)}).Days(); len(got) != 0 {
			t.Fatalf("Days() = %v, want none", got)
		}
	})
}

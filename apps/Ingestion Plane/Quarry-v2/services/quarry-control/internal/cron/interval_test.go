package cron

import (
	"testing"
	"time"
)

// TestMinInterval_Cadences pins the smallest gap for the shapes the schedule
// floor depends on. The floor (5 minutes for scrape, 15 for crawl and batch)
// is decided on these numbers.
func TestMinInterval_Cadences(t *testing.T) {
	cases := []struct {
		expr string
		want time.Duration
	}{
		{"*/5 * * * *", 5 * time.Minute},
		{"*/15 * * * *", 15 * time.Minute},
		{"0 * * * *", time.Hour},
		{"0 9 * * *", 24 * time.Hour},
		{"0 9 * * 1", 7 * 24 * time.Hour},
		{"0-30/10 * * * *", 10 * time.Minute},
		{"0,30 * * * *", 30 * time.Minute},
	}
	for _, tc := range cases {
		got, err := MinInterval(tc.expr, time.UTC)
		if err != nil {
			t.Fatalf("MinInterval(%q): %v", tc.expr, err)
		}
		if got != tc.want {
			t.Errorf("MinInterval(%q) = %s, want %s", tc.expr, got, tc.want)
		}
	}
}

// TestMinInterval_NilLocationIsUTC: a nil location reads the expression in UTC.
func TestMinInterval_NilLocationIsUTC(t *testing.T) {
	got, err := MinInterval("*/5 * * * *", nil)
	if err != nil || got != 5*time.Minute {
		t.Fatalf("MinInterval(nil) = %s, %v; want 5m", got, err)
	}
}

// TestMinInterval_TooFrequentIsBelowFloor: a 5-minute scrape cadence is at the
// floor; a 10-minute crawl cadence is below the 15-minute floor the caller uses.
func TestMinInterval_TooFrequentIsBelowFloor(t *testing.T) {
	gap, err := MinInterval("*/10 * * * *", time.UTC)
	if err != nil {
		t.Fatal(err)
	}
	if gap >= 15*time.Minute {
		t.Fatalf("gap=%s, want below 15m", gap)
	}
}

// TestMinInterval_InvalidExpressionIsAnError: validation runs first, so
// unsupported syntax never reaches the sampler.
func TestMinInterval_InvalidExpressionIsAnError(t *testing.T) {
	for _, expr := range []string{"", "@daily", "0 0 * *", "0 0 L * *", "*/0 * * * *"} {
		if _, err := MinInterval(expr, time.UTC); err == nil {
			t.Errorf("MinInterval(%q) = nil error, want an error", expr)
		}
	}
}

// TestMinInterval_NeverFiresIsAnError: 30 February is a valid field set that
// no date matches. The search gives up within its horizon instead of hanging.
func TestMinInterval_NeverFiresIsAnError(t *testing.T) {
	if _, err := MinInterval("0 0 30 2 *", time.UTC); err == nil {
		t.Fatal("MinInterval(30 Feb) = nil error, want never-fires")
	}
}

// TestMinInterval_DayRule: with both day fields restricted the classic rule
// matches either one, so "0 0 13 * 5" fires on the 13th and on Fridays.
func TestMinInterval_DayRule(t *testing.T) {
	sp, err := compile("0 0 13 * 5")
	if err != nil {
		t.Fatal(err)
	}
	friday := time.Date(2026, time.January, 2, 0, 0, 0, 0, time.UTC) // a Friday
	if friday.Weekday() != time.Friday {
		t.Fatalf("fixture: %s is not a Friday", friday.Weekday())
	}
	if !sp.matches(friday) {
		t.Error("a Friday that is not the 13th must match")
	}
	thirteenth := time.Date(2026, time.January, 13, 0, 0, 0, 0, time.UTC) // a Tuesday
	if !sp.matches(thirteenth) {
		t.Error("the 13th that is not a Friday must match")
	}
}

package cron

import (
	"testing"
	"time"
)

func mustNext(t *testing.T, expr string, from time.Time) time.Time {
	t.Helper()
	s, err := Parse(expr)
	if err != nil {
		t.Fatalf("Parse(%q) error: %v", expr, err)
	}
	next, err := s.Next(from)
	if err != nil {
		t.Fatalf("Next(%q) error: %v", expr, err)
	}
	return next
}

func TestNextEveryMinute(t *testing.T) {
	from := time.Date(2026, 7, 15, 9, 30, 20, 0, time.UTC)
	got := mustNext(t, "* * * * *", from)
	want := time.Date(2026, 7, 15, 9, 31, 0, 0, time.UTC)
	if !got.Equal(want) {
		t.Fatalf("every-minute next = %v, want %v", got, want)
	}
}

func TestNextDailyAtNine(t *testing.T) {
	// 0 9 * * *  → next 09:00. From 09:30 → tomorrow 09:00.
	from := time.Date(2026, 7, 15, 9, 30, 0, 0, time.UTC)
	got := mustNext(t, "0 9 * * *", from)
	want := time.Date(2026, 7, 16, 9, 0, 0, 0, time.UTC)
	if !got.Equal(want) {
		t.Fatalf("daily@9 next = %v, want %v", got, want)
	}
}

func TestNextWeekdayMonday(t *testing.T) {
	// 0 9 * * 1 → Monday 09:00. 2026-07-15 is a Wednesday → next Monday is 07-20.
	from := time.Date(2026, 7, 15, 12, 0, 0, 0, time.UTC)
	got := mustNext(t, "0 9 * * 1", from)
	want := time.Date(2026, 7, 20, 9, 0, 0, 0, time.UTC)
	if !got.Equal(want) {
		t.Fatalf("monday@9 next = %v (weekday %v), want %v", got, got.Weekday(), want)
	}
	if got.Weekday() != time.Monday {
		t.Fatalf("expected Monday, got %v", got.Weekday())
	}
}

func TestNextStepAndRange(t *testing.T) {
	// */15 9-17 * * *  → every 15 min during business hours. From 09:07 → 09:15.
	from := time.Date(2026, 7, 15, 9, 7, 0, 0, time.UTC)
	got := mustNext(t, "*/15 9-17 * * *", from)
	want := time.Date(2026, 7, 15, 9, 15, 0, 0, time.UTC)
	if !got.Equal(want) {
		t.Fatalf("step/range next = %v, want %v", got, want)
	}
}

func TestNextListMinutes(t *testing.T) {
	// 0,30 * * * *  → top and bottom of the hour. From 09:05 → 09:30.
	from := time.Date(2026, 7, 15, 9, 5, 0, 0, time.UTC)
	got := mustNext(t, "0,30 * * * *", from)
	want := time.Date(2026, 7, 15, 9, 30, 0, 0, time.UTC)
	if !got.Equal(want) {
		t.Fatalf("list next = %v, want %v", got, want)
	}
}

func TestSundayBothSpellings(t *testing.T) {
	from := time.Date(2026, 7, 15, 0, 0, 0, 0, time.UTC) // Wednesday
	// 0 0 * * 0 and 0 0 * * 7 must both resolve to the next Sunday 00:00.
	got0 := mustNext(t, "0 0 * * 0", from)
	got7 := mustNext(t, "0 0 * * 7", from)
	if !got0.Equal(got7) {
		t.Fatalf("Sunday 0 vs 7 differ: %v vs %v", got0, got7)
	}
	if got0.Weekday() != time.Sunday {
		t.Fatalf("expected Sunday, got %v", got0.Weekday())
	}
}

func TestParseRejectsMalformed(t *testing.T) {
	for _, expr := range []string{
		"", "* * * *", "* * * * * *", "60 * * * *", "* 24 * * *",
		"* * 0 * *", "* * * 13 *", "*/0 * * * *", "5-1 * * * *", "abc * * * *",
	} {
		if _, err := Parse(expr); err == nil {
			t.Errorf("Parse(%q) should have failed", expr)
		}
	}
}

func TestDomOrDowWhenBothRestricted(t *testing.T) {
	// 0 0 1 * 1 → fires on the 1st OR any Monday (Vixie OR semantics).
	s, err := Parse("0 0 1 * 1")
	if err != nil {
		t.Fatal(err)
	}
	// From Wed 2026-07-15: next Monday is 07-20; the 1st is 08-01. OR => 07-20.
	got, _ := s.Next(time.Date(2026, 7, 15, 12, 0, 0, 0, time.UTC))
	want := time.Date(2026, 7, 20, 0, 0, 0, 0, time.UTC)
	if !got.Equal(want) {
		t.Fatalf("dom-or-dow next = %v, want %v", got, want)
	}
}

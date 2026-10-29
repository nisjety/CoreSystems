package main

import (
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// T43: a stored retry instant that is still ahead refuses the start before any
// Doffin call. The decision is pure, so it needs no database. ok means a run
// stored an instant, not that the instant is ahead: LastRetryAfter returns ok for
// past instants too (D-B4), so the local clock alone decides whether it is ahead.
func TestStartRefusesBeforeRetryAfter(t *testing.T) {
	now := time.Date(2026, time.October, 9, 12, 0, 0, 0, time.UTC)
	cases := []struct {
		name  string
		until time.Time
		ok    bool
		want  bool
	}{
		{"stored instant still ahead: refuse", now.Add(90 * time.Second), true, true},
		{"no stored instant: start", time.Time{}, false, false},
		{"no stored instant, so nothing refuses even with a future instant: start", now.Add(time.Minute), false, false},
		{"stored instant passed on the local clock: start", now.Add(-time.Second), false, false},
		{"stored instant the local clock has passed, ok from the store: start", now.Add(-time.Second), true, false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := startRefused(c.until, c.ok, now); got != c.want {
				t.Errorf("startRefused(%v, %v, now) = %v, want %v", c.until, c.ok, got, c.want)
			}
		})
	}
}

// Without -to the window ends at UTC yesterday, which is the day before the UTC
// calendar date, not the local one.
func TestStartDefaultsToYesterdayUTC(t *testing.T) {
	// 00:30 on 9 October in UTC+2 is 22:30 on 8 October in UTC.
	now := time.Date(2026, time.October, 9, 0, 30, 0, 0, time.FixedZone("UTC+2", 2*3600))
	req, err := parseArgs([]string{"-from", "2026-10-01"}, now)
	if err != nil {
		t.Fatalf("parseArgs: %v", err)
	}
	wantFrom := time.Date(2026, time.October, 1, 0, 0, 0, 0, time.UTC)
	wantTo := time.Date(2026, time.October, 7, 0, 0, 0, 0, time.UTC)
	if !req.window.From.Equal(wantFrom) || !req.window.To.Equal(wantTo) {
		t.Errorf("window = %v to %v, want %v to %v", req.window.From, req.window.To, wantFrom, wantTo)
	}
	if req.mode != "sync" {
		t.Errorf("mode = %q, want sync", req.mode)
	}
}

// -census selects census mode, and the flags reach the request unchanged.
func TestStartReadsEveryFlag(t *testing.T) {
	now := time.Date(2026, time.October, 9, 12, 0, 0, 0, time.UTC)
	req, err := parseArgs([]string{"-from", "2026-10-01", "-to", "2026-10-03", "-recheck", "4", "-max-downloads", "7", "-census"}, now)
	if err != nil {
		t.Fatalf("parseArgs: %v", err)
	}
	if req.mode != "census" || req.recheck != 4 || req.maxDownloads != 7 {
		t.Errorf("request = mode %q recheck %d max %d, want census 4 7", req.mode, req.recheck, req.maxDownloads)
	}
	if got := req.window.To.Format("2006-01-02"); got != "2026-10-03" {
		t.Errorf("to = %s, want 2026-10-03", got)
	}
}

// Every flag of the request reaches the engine's options, and the progress callback is
// the one the caller gave. Nothing else in the CLI would notice a flag that was parsed
// and then dropped on the way to the engine.
func TestEngineOptionsCarryEveryFlag(t *testing.T) {
	req := request{mode: doffinsync.ModeCensus, recheck: 4, maxDownloads: 7, pageSize: 25}
	var reported doffinsync.DayOutcome
	opts := engineOptions(req, func(d doffinsync.DayOutcome) { reported = d })

	if opts.Mode != doffinsync.ModeCensus || opts.Recheck != 4 || opts.MaxDownloads != 7 || opts.PageSize != 25 {
		t.Errorf("options = mode %q recheck %d max %d page size %d, want census 4 7 25",
			opts.Mode, opts.Recheck, opts.MaxDownloads, opts.PageSize)
	}
	if opts.Progress == nil {
		t.Fatal("options carry no progress callback")
	}
	opts.Progress(doffinsync.DayOutcome{Searches: 3})
	if reported.Searches != 3 {
		t.Errorf("the progress callback did not reach the caller's function: %+v", reported)
	}
}

// -page-size reaches the request, and its absence leaves it at 0, which is the engine's
// default of 100.
func TestStartReadsPageSize(t *testing.T) {
	now := time.Date(2026, time.October, 9, 12, 0, 0, 0, time.UTC)
	req, err := parseArgs([]string{"-from", "2026-10-01", "-page-size", "25"}, now)
	if err != nil {
		t.Fatalf("parseArgs: %v", err)
	}
	if req.pageSize != 25 {
		t.Errorf("pageSize = %d, want 25", req.pageSize)
	}
	req, err = parseArgs([]string{"-from", "2026-10-01"}, now)
	if err != nil {
		t.Fatalf("parseArgs: %v", err)
	}
	if req.pageSize != 0 {
		t.Errorf("pageSize = %d without the flag, want 0 for the default", req.pageSize)
	}
}

// Invalid input exits 1 and never echoes the value. The flag package quotes
// rejected values in its own errors, so the check covers the returned error too.
func TestStartRejectsBadArgsWithoutEcho(t *testing.T) {
	now := time.Date(2026, time.October, 9, 12, 0, 0, 0, time.UTC)
	cases := []struct {
		name  string
		args  []string
		value string
	}{
		{"malformed from", []string{"-from", "2026-13-45"}, "2026-13-45"},
		{"missing from", []string{"-to", "2026-10-07"}, ""},
		{"impossible to", []string{"-from", "2026-10-01", "-to", "2026-02-30"}, "2026-02-30"},
		{"to is not a closed day", []string{"-from", "2026-10-01", "-to", "2099-01-01"}, "2099-01-01"},
		{"from before the corpus", []string{"-from", "2016-12-31"}, "2016-12-31"},
		{"from after to", []string{"-from", "2026-10-05", "-to", "2026-10-02"}, "2026-10-05"},
		{"non-numeric recheck", []string{"-from", "2026-10-01", "-recheck", "seven-x"}, "seven-x"},
		{"negative max downloads", []string{"-from", "2026-10-01", "-max-downloads", "-42"}, "-42"},
		{"page size above the API limit", []string{"-from", "2026-10-01", "-page-size", "101"}, "101"},
		{"negative page size", []string{"-from", "2026-10-01", "-page-size", "-7"}, "-7"},
		{"non-numeric page size", []string{"-from", "2026-10-01", "-page-size", "twenty-x"}, "twenty-x"},
		{"unknown flag with a value", []string{"-from", "2026-10-01", "-token", "tok-abc"}, "tok-abc"},
		{"positional argument", []string{"-from", "2026-10-01", "extra-arg-9"}, "extra-arg-9"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			_, err := parseArgs(c.args, now)
			if err == nil {
				t.Fatal("parseArgs accepted input it must refuse")
			}
			if c.value != "" && strings.Contains(err.Error(), c.value) {
				t.Errorf("error %q echoes the rejected value", err.Error())
			}
		})
	}
}

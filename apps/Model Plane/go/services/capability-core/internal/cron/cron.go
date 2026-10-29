// Package cron parses standard 5-field cron expressions ("min hour dom month
// dow") and computes the next fire time. Dependency-free (no external cron lib
// in go.mod) so the sweeper can advance schedules deterministically.
//
// Supported per field: `*`, `a`, `a-b`, `*/n`, `a-b/n`, and comma lists of any
// of these. Day-of-week is 0-7 (0 and 7 both Sunday). Day-of-month vs
// day-of-week follows Vixie cron: when BOTH are restricted the match is an OR;
// otherwise the restricted one (if any) applies.
package cron

import (
	"errors"
	"fmt"
	"strconv"
	"strings"
	"time"
)

type field struct {
	allowed map[int]bool
	star    bool
}

// Schedule is a parsed cron expression.
type Schedule struct {
	min, hour, dom, month, dow field
}

const maxYearsAhead = 5

// Parse parses a 5-field cron expression. Returns an error on malformed input.
func Parse(expr string) (*Schedule, error) {
	parts := strings.Fields(strings.TrimSpace(expr))
	if len(parts) != 5 {
		return nil, fmt.Errorf("cron expression must have 5 fields, got %d", len(parts))
	}
	min, err := parseField(parts[0], 0, 59)
	if err != nil {
		return nil, fmt.Errorf("minute field: %w", err)
	}
	hour, err := parseField(parts[1], 0, 23)
	if err != nil {
		return nil, fmt.Errorf("hour field: %w", err)
	}
	dom, err := parseField(parts[2], 1, 31)
	if err != nil {
		return nil, fmt.Errorf("day-of-month field: %w", err)
	}
	month, err := parseField(parts[3], 1, 12)
	if err != nil {
		return nil, fmt.Errorf("month field: %w", err)
	}
	dow, err := parseField(parts[4], 0, 7)
	if err != nil {
		return nil, fmt.Errorf("day-of-week field: %w", err)
	}
	// Normalize Sunday: 7 => 0.
	if dow.allowed[7] {
		dow.allowed[0] = true
		delete(dow.allowed, 7)
	}
	return &Schedule{min: min, hour: hour, dom: dom, month: month, dow: dow}, nil
}

func parseField(token string, lo, hi int) (field, error) {
	f := field{allowed: map[int]bool{}}
	for part := range strings.SplitSeq(token, ",") {
		part = strings.TrimSpace(part)
		if part == "" {
			return f, errors.New("empty term")
		}
		step := 1
		rangePart := part
		if base, stepStr, ok := strings.Cut(part, "/"); ok {
			var err error
			step, err = strconv.Atoi(stepStr)
			if err != nil || step < 1 {
				return f, fmt.Errorf("invalid step %q", stepStr)
			}
			rangePart = base
		}
		start, end := lo, hi
		if rangePart == "*" {
			f.star = true
		} else if startStr, endStr, ok := strings.Cut(rangePart, "-"); ok {
			var err error
			start, err = strconv.Atoi(startStr)
			if err != nil {
				return f, fmt.Errorf("invalid range start %q", startStr)
			}
			end, err = strconv.Atoi(endStr)
			if err != nil {
				return f, fmt.Errorf("invalid range end %q", endStr)
			}
		} else {
			v, err := strconv.Atoi(rangePart)
			if err != nil {
				return f, fmt.Errorf("invalid value %q", rangePart)
			}
			start, end = v, v
		}
		if start < lo || end > hi || start > end {
			return f, fmt.Errorf("value out of range [%d-%d]: %q", lo, hi, part)
		}
		for v := start; v <= end; v += step {
			f.allowed[v] = true
		}
	}
	if len(f.allowed) == 0 {
		return f, errors.New("no values")
	}
	return f, nil
}

// dayMatches applies the Vixie cron dom/dow rule.
func (s *Schedule) dayMatches(t time.Time) bool {
	domOK := s.dom.allowed[t.Day()]
	dowOK := s.dow.allowed[int(t.Weekday())] // time.Weekday: Sunday=0
	switch {
	case s.dom.star && s.dow.star:
		return true
	case s.dom.star:
		return dowOK
	case s.dow.star:
		return domOK
	default:
		return domOK || dowOK
	}
}

// Next returns the earliest time strictly after `after` (in after's location)
// that matches the schedule, at minute resolution. Returns an error if no match
// occurs within maxYearsAhead (guards against impossible expressions).
func (s *Schedule) Next(after time.Time) (time.Time, error) {
	loc := after.Location()
	// Start at the next whole minute.
	t := after.Truncate(time.Minute).Add(time.Minute)
	limit := after.AddDate(maxYearsAhead, 0, 0)
	for t.Before(limit) {
		if !s.month.allowed[int(t.Month())] {
			// Jump to the first day of the next month at 00:00.
			t = time.Date(t.Year(), t.Month(), 1, 0, 0, 0, 0, loc).AddDate(0, 1, 0)
			continue
		}
		if !s.dayMatches(t) {
			// Jump to 00:00 of the next day.
			t = time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, loc).AddDate(0, 0, 1)
			continue
		}
		if !s.hour.allowed[t.Hour()] {
			// Jump to the next hour at :00.
			t = time.Date(t.Year(), t.Month(), t.Day(), t.Hour(), 0, 0, 0, loc).Add(time.Hour)
			continue
		}
		if !s.min.allowed[t.Minute()] {
			t = t.Add(time.Minute)
			continue
		}
		return t, nil
	}
	return time.Time{}, fmt.Errorf("no cron match within %d years", maxYearsAhead)
}

// NextFrom is a convenience that parses `expr` in `tz` (falling back to UTC) and
// returns the next fire strictly after `after`.
func NextFrom(expr, tz string, after time.Time) (time.Time, error) {
	sched, err := Parse(expr)
	if err != nil {
		return time.Time{}, err
	}
	loc, err := time.LoadLocation(tz)
	if err != nil || tz == "" {
		loc = time.UTC
	}
	next, err := sched.Next(after.In(loc))
	if err != nil {
		return time.Time{}, err
	}
	return next.UTC(), nil
}

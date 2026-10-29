package main

import (
	"encoding/json"
	"io"
	"time"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// dayLine is one touched day: the DayOutcome fields and nothing else. It carries
// no notice id, no body and no contact field (T40).
type dayLine struct {
	Day        string `json:"day"`
	State      string `json:"state"`
	ErrorCode  string `json:"error_code,omitempty"`
	Total      int64  `json:"total"`
	Accessible int64  `json:"accessible"`
	Held       int    `json:"held"`
	Stored     int    `json:"stored"`
	NotFound   int    `json:"not_found"`
	Rejected   int    `json:"rejected"` // terminal rejections: refused for their content three times
	Unresolved int    `json:"unresolved"`
	Searches   int    `json:"searches"` // the day's own searches: a census reads the sweeps from it
}

// countersLine is the run totals as counts.
type countersLine struct {
	Searches    int `json:"searches"`
	Downloads   int `json:"downloads"`
	Stored      int `json:"stored"`
	Duplicates  int `json:"duplicates"`
	HeldSkipped int `json:"held_skipped"`
	NotFound    int `json:"not_found"`
}

// summaryLine closes the report: the run id, the reason, the counts and, when a
// 429 closed the gate, the instant it reopens. A refused start has run id 0.
type summaryLine struct {
	RunID           int64        `json:"run_id"`
	Reason          string       `json:"reason"`
	Counters        countersLine `json:"counters"`
	RetryAfterUntil *time.Time   `json:"retry_after_until,omitempty"`
}

func newDayLine(d doffinsync.DayOutcome) dayLine {
	return dayLine{
		Day:        d.Day.Format(dateLayout),
		State:      string(d.State),
		ErrorCode:  string(d.ErrorCode),
		Total:      d.Total,
		Accessible: d.Accessible,
		Held:       d.Held,
		Stored:     d.Stored,
		NotFound:   d.NotFound,
		Rejected:   d.Rejected,
		Unresolved: d.Unresolved,
		Searches:   d.Searches,
	}
}

func newSummary(runID int64, reason doffinsync.Reason, c doffinsync.Counters, retry *time.Time) summaryLine {
	return summaryLine{
		RunID:  runID,
		Reason: string(reason),
		Counters: countersLine{
			Searches:    c.Searches,
			Downloads:   c.Downloads,
			Stored:      c.Stored,
			Duplicates:  c.Duplicates,
			HeldSkipped: c.HeldSkipped,
			NotFound:    c.NotFound,
		},
		RetryAfterUntil: retry,
	}
}

// report writes one JSON object per line to stdout. It keeps the first write
// error, so a broken stdout is reported once rather than once per line.
type report struct {
	enc *json.Encoder
	err error
}

func newReport(w io.Writer) *report {
	return &report{enc: json.NewEncoder(w)}
}

func (r *report) emit(v any) {
	if r.err == nil {
		r.err = r.enc.Encode(v)
	}
}

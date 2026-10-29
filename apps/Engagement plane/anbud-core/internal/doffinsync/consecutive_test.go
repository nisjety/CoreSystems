package doffinsync

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// TestSuccessfulEmptyDayResetsConsecutiveCount (E2a, D-B1): a successful search
// page resets the count. Four failing days, a success and four more failing days
// are never five in a row, so the run must reach the last day.
func TestSuccessfulEmptyDayResetsConsecutiveCount(t *testing.T) {
	t.Parallel()

	h := newHarness()
	var days []time.Time
	for d := day(2026, 9, 30); !d.After(day(2026, 10, 8)); d = d.AddDate(0, 0, 1) {
		days = append(days, d)
	}
	empty := days[4] // 2026-10-04 succeeds with no hits
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if p.IssueDateFrom.Equal(empty) {
			return searchResp(0, 0, nil), nil
		}
		return doffin.SearchResponse{}, &doffin.HTTPError{Operation: "search", Status: 503, Message: "busy"}
	}
	sum := h.run(t, h.engine(t, ModeSync), days[0], days[len(days)-1])

	switch {
	case sum.Reason == ReasonFailed:
		t.Errorf("run reason = failed after %d day outcomes: the success must reset the count", len(sum.Days))
	case len(sum.Days) != len(days):
		t.Errorf("day outcomes = %d, want %d: the run must reach the last day", len(sum.Days), len(days))
	}
}

// TestTooLargeNoticesDoNotStopTheRun (E2b, D-B1, D-B2): a too-large notice is a
// content refusal, not an upstream error. Ten in a row are all attempted, and the
// run ends partial, not failed.
func TestTooLargeNoticesDoNotStopTheRun(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d1, d2 := day(2026, 10, 6), day(2026, 10, 7)
	all := ids(1, 10)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if p.IssueDateFrom.Equal(d1) {
			return searchResp(10, 10, all), nil
		}
		return searchResp(0, 0, nil), nil
	}
	h.api.download = func(context.Context, string) (doffin.Notice, error) {
		return doffin.Notice{}, doffin.ErrResponseTooLarge
	}
	sum := h.run(t, h.engine(t, ModeSync), d1, d2)

	tooLarge := 0
	for _, a := range h.store.attempts {
		if a.Outcome == OutcomeTooLarge {
			tooLarge++
		}
	}
	rec := h.finishedDay(t, d1)
	switch {
	case len(h.api.downloads) != 10:
		t.Errorf("downloads = %d, want all 10: content refusals must not stop the day", len(h.api.downloads))
	case tooLarge != 10 || len(h.store.attempts) != 10:
		t.Errorf("attempts = %d of which too_large %d, want 10 too_large", len(h.store.attempts), tooLarge)
	case sum.Reason != ReasonPartial:
		t.Errorf("run reason = %s, want partial", sum.Reason)
	case rec.State != StatePartial || rec.ErrorCode != CodeItemRejected || rec.Unresolved != 10:
		t.Errorf("day = %s/%s unresolved %d, want partial/item_rejected with 10 unresolved", rec.State, rec.ErrorCode, rec.Unresolved)
	}
}

// TestTransportErrorsAroundASuccessDoNotStopTheRun (E2c, D-B1): four transport
// errors, a successful download and four more are never five in a row.
func TestTransportErrorsAroundASuccessDoNotStopTheRun(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d := day(2026, 10, 7)
	all := ids(1, 9)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(9, 9, all), nil
	}
	calls := 0
	h.api.download = func(_ context.Context, id string) (doffin.Notice, error) {
		calls++
		if calls == 5 {
			return notice(id, []byte("ok "+id)), nil
		}
		return doffin.Notice{}, errors.New("connection reset by peer")
	}
	sum := h.run(t, h.engine(t, ModeSync), d, d)

	transport := 0
	for _, a := range h.store.attempts {
		if a.Outcome == OutcomeTransportError {
			transport++
		}
	}
	switch {
	case sum.Reason != ReasonPartial:
		t.Errorf("run reason = %s, want partial: four, a success, four must not stop the run", sum.Reason)
	case len(h.api.downloads) != 9:
		t.Errorf("downloads = %d, want 9", len(h.api.downloads))
	case transport != 8:
		t.Errorf("transport_error attempts = %d, want 8", transport)
	}
}

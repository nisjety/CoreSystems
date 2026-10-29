package doffinsync

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// T26
func TestConsecutiveErrorsStopTheRun(t *testing.T) {
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
		return doffin.Notice{}, &doffin.HTTPError{Operation: "download", Status: 503, Message: "busy"}
	}
	sum := h.run(t, h.engine(t, ModeSync), d1, d2)

	if len(h.api.downloads) != 5 {
		t.Errorf("downloads = %d, want exactly 5", len(h.api.downloads))
	}
	if sum.Reason != ReasonFailed {
		t.Errorf("run reason = %s, want failed", sum.Reason)
	}
	if rec := h.finishedDay(t, d1); rec.State != StateFailed || rec.ErrorCode != CodeConsecutiveErrors {
		t.Errorf("day = %s/%s, want failed/consecutive_errors", rec.State, rec.ErrorCode)
	}
	if got := h.api.searchDays(); len(got) != 2 {
		t.Errorf("searches = %d, want 2: two passes on the first day, the next day untouched", len(got))
	}
}

// T27
func TestSuccessResetsConsecutiveCount(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d1, d2 := day(2026, 10, 6), day(2026, 10, 7)
	all := ids(1, 9)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if p.IssueDateFrom.Equal(d1) {
			return searchResp(9, 9, all), nil
		}
		return searchResp(0, 0, nil), nil
	}
	calls := 0
	h.api.download = func(_ context.Context, id string) (doffin.Notice, error) {
		calls++
		if calls == 5 {
			return notice(id, []byte("ok "+id)), nil
		}
		return doffin.Notice{}, &doffin.HTTPError{Operation: "download", Status: 503, Message: "busy"}
	}
	sum := h.run(t, h.engine(t, ModeSync), d1, d2)

	switch {
	case sum.Reason != ReasonPartial:
		t.Errorf("run reason = %s, want partial: four, a success, four must not stop the run", sum.Reason)
	case len(h.api.downloads) != 9:
		t.Errorf("downloads = %d, want 9", len(h.api.downloads))
	case len(h.api.searchDays()) != 4:
		t.Errorf("searches = %d, want 4: two passes on each of the two days", len(h.api.searchDays()))
	}
}

// T28
func TestUnauthorizedStopsAtFirstCall(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d1, d2 := day(2026, 10, 6), day(2026, 10, 7)
	all := ids(1, 3)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if p.IssueDateFrom.Equal(d1) {
			return searchResp(3, 3, all), nil
		}
		return searchResp(0, 0, nil), nil
	}
	h.api.download = func(context.Context, string) (doffin.Notice, error) {
		return doffin.Notice{}, doffin.ErrUnauthorized
	}
	sum := h.run(t, h.engine(t, ModeSync), d1, d2)

	switch {
	case len(h.api.downloads) != 1:
		t.Errorf("downloads = %d, want 1", len(h.api.downloads))
	case len(h.api.searchDays()) != 2:
		t.Errorf("searches = %d, want 2: two passes before the first download", len(h.api.searchDays()))
	case sum.Reason != ReasonKeyRejected:
		t.Errorf("run reason = %s, want key_rejected", sum.Reason)
	}
	if rec := h.finishedDay(t, d1); rec.State != StateFailed || rec.ErrorCode != CodeUnauthorized {
		t.Errorf("day = %s/%s, want failed/unauthorized", rec.State, rec.ErrorCode)
	}
	if len(h.store.attempts) != 1 || h.store.attempts[0].Outcome != OutcomeUnauthorized {
		t.Errorf("attempts = %+v, want one unauthorized row", h.store.attempts)
	}
}

// T29
func TestRateLimitPausesRunAndRecordsWait(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d1, d2 := day(2026, 10, 6), day(2026, 10, 7)
	all := ids(1, 1)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if p.IssueDateFrom.Equal(d1) {
			return searchResp(1, 1, all), nil
		}
		return searchResp(0, 0, nil), nil
	}
	h.api.download = func(context.Context, string) (doffin.Notice, error) {
		return doffin.Notice{}, &doffin.RateLimitedError{Operation: "download", RetryAfter: 90 * time.Second}
	}
	sum := h.run(t, h.engine(t, ModeSync), d1, d2)

	switch {
	case len(h.api.searchDays()) != 2 || len(h.api.downloads) != 1:
		t.Errorf("calls = %d searches and %d downloads, want 2 searches (two passes) and 1 download", len(h.api.searchDays()), len(h.api.downloads))
	case sum.Reason != ReasonPaused:
		t.Errorf("run reason = %s, want paused", sum.Reason)
	case sum.RetryAfterUntil == nil || !sum.RetryAfterUntil.Equal(testNow.Add(90*time.Second)):
		t.Errorf("RetryAfterUntil = %v, want now plus 90s", sum.RetryAfterUntil)
	}
	if rec := h.finishedDay(t, d1); rec.State != StatePaused || rec.ErrorCode != CodeRateLimited {
		t.Errorf("day = %s/%s, want paused/rate_limited", rec.State, rec.ErrorCode)
	}
}

// T30
func TestSearchNotFoundIsInvalidParamsAndStops(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d1, d2 := day(2026, 10, 6), day(2026, 10, 7)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return doffin.SearchResponse{}, doffin.ErrNotFound
	}
	sum := h.run(t, h.engine(t, ModeSync), d1, d2)

	if got := len(h.api.searchDays()); got != 1 {
		t.Errorf("searches = %d, want 1: a bad request must not loop across days", got)
	}
	if sum.Reason != ReasonFailed {
		t.Errorf("run reason = %s, want failed", sum.Reason)
	}
	if rec := h.finishedDay(t, d1); rec.State != StateFailed || rec.ErrorCode != CodeInvalidParams {
		t.Errorf("day = %s/%s, want failed/invalid_params", rec.State, rec.ErrorCode)
	}
}

// T31
func TestDecodeErrorIsNeverADoneDay(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d := day(2026, 10, 7)
	raw := "<html>maintenance</html>"
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		resp := doffin.SearchResponse{
			Raw:         json.RawMessage(raw),
			ContentType: "text/html; charset=utf-8",
			FetchedAt:   testNow,
		}
		return resp, &doffin.DecodeError{Operation: "search", Err: errors.New("invalid character '<'")}
	}
	h.run(t, h.engine(t, ModeSync), d, d)

	if len(h.store.pages) != 1 {
		t.Fatalf("PutSearchPage calls = %d, want 1", len(h.store.pages))
	}
	p := h.store.pages[0]
	if string(p.Body) != raw || p.ContentType != "text/html; charset=utf-8" || p.Page != 1 {
		t.Errorf("stored page = page %d %q %q, want the exact body and content type", p.Page, p.ContentType, p.Body)
	}
	rec := h.finishedDay(t, d)
	switch {
	case rec.State != StateFailed || rec.ErrorCode != CodeDecodeError:
		t.Errorf("day = %s/%s, want failed/decode_error", rec.State, rec.ErrorCode)
	case len(h.api.downloads) != 0:
		t.Errorf("downloads = %d, want none", len(h.api.downloads))
	}
}

// T32
func TestEmptySearchBodyIsNotStored(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d := day(2026, 10, 7)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return doffin.SearchResponse{}, &doffin.DecodeError{Operation: "search", Err: errors.New("EOF")}
	}
	h.run(t, h.engine(t, ModeSync), d, d)

	if len(h.store.pages) != 0 {
		t.Errorf("PutSearchPage calls = %d, want none for an empty body", len(h.store.pages))
	}
	if rec := h.finishedDay(t, d); rec.State != StateFailed || rec.ErrorCode != CodeDecodeError {
		t.Errorf("day = %s/%s, want failed/decode_error", rec.State, rec.ErrorCode)
	}
}

// T33
func TestCancellationIsInterruptedWithoutAttemptRow(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d := day(2026, 10, 7)
	all := ids(1, 3)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(3, 3, all), nil
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	h.api.download = func(c context.Context, _ string) (doffin.Notice, error) {
		cancel()
		return doffin.Notice{}, c.Err()
	}
	sum, err := h.engine(t, ModeSync).Run(ctx, Window{From: d, To: d})
	if err != nil {
		t.Fatalf("Run: %v", err)
	}

	rec := h.finishedDay(t, d)
	switch {
	case rec.State != StateInterrupted || rec.ErrorCode != CodeCancelled:
		t.Errorf("day = %s/%s, want interrupted/cancelled", rec.State, rec.ErrorCode)
	case sum.Reason != ReasonCancelled:
		t.Errorf("run reason = %s, want cancelled", sum.Reason)
	case len(h.store.attempts) != 0:
		t.Errorf("attempts = %+v, want none: a cancelled call has no response", h.store.attempts)
	case len(h.api.downloads) != 1:
		t.Errorf("downloads = %d, want 1: the run must stop at once", len(h.api.downloads))
	}
	if len(h.store.finishCtxErrs) == 0 {
		t.Fatal("FinishDay was never called")
	}
	for i, e := range h.store.finishCtxErrs {
		if e != nil {
			t.Errorf("FinishDay call %d received a context that already ended: %v", i, e)
		}
	}
}

// T41
func TestStopMessagesAreFixedText(t *testing.T) {
	t.Parallel()

	const marker = "SECRET-MARKER-7f3a"
	h := newHarness()
	d1, d2 := day(2026, 10, 6), day(2026, 10, 7)
	all := ids(1, 2)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if p.IssueDateFrom.Equal(d1) {
			return searchResp(2, 2, all), nil
		}
		return doffin.SearchResponse{}, fmt.Errorf("search https://api.doffin.no/x?k=%s: reset", marker)
	}
	h.api.download = func(context.Context, string) (doffin.Notice, error) {
		return doffin.Notice{}, fmt.Errorf("dial https://api.doffin.no/public/v2/download/x?k=%s: reset", marker)
	}
	sum := h.run(t, h.engine(t, ModeSync), d1, d2)

	// Without recorded evidence the checks below would pass vacuously.
	if len(h.store.attempts) == 0 || len(h.store.finished) == 0 || len(h.store.runOutcomes) == 0 || len(sum.Days) == 0 {
		t.Fatalf("attempts %d, day records %d, run outcomes %d, days in summary %d: nothing was recorded to check",
			len(h.store.attempts), len(h.store.finished), len(h.store.runOutcomes), len(sum.Days))
	}
	mustNotContain(t, "attempts", h.store.attempts, marker)
	mustNotContain(t, "day records", h.store.finished, marker)
	mustNotContain(t, "run outcomes", h.store.runOutcomes, marker)
	mustNotContain(t, "summary", sum, marker)
}

// mustNotContain fails if the printed form of v contains marker.
func mustNotContain(t *testing.T, label string, v any, marker string) {
	t.Helper()
	if s := fmt.Sprintf("%+v", v); strings.Contains(s, marker) {
		t.Errorf("%s carry an error's text: %s", label, s)
	}
}

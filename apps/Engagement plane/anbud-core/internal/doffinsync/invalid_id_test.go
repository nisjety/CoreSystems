package doffinsync

import (
	"context"
	"fmt"
	"slices"
	"testing"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// malformedID is a search hit whose id is outside the Doffin shape. The client
// refuses such an id before any HTTP call with doffin.ErrInvalidParams (D-A), so
// these tests answer it the same way.
func malformedID(i int) string {
	return fmt.Sprintf("not-a-notice-%d", i)
}

// upstream503 is a download failure that counts toward the five-in-a-row limit.
func upstream503() error {
	return &doffin.HTTPError{Operation: "download", Status: 503, Message: "busy"}
}

// TestMalformedIDRecordsNoAttemptAndValidIDsDownload (D-A, case a): one malformed
// id among valid ones leaves no attempt row, the valid ids are still stored, and
// the day ends partial with one item_rejected.
func TestMalformedIDRecordsNoAttemptAndValidIDsDownload(t *testing.T) {
	t.Parallel()

	h := newHarness()
	valid := ids(1, 2)
	bad := malformedID(1)
	hits := []string{valid[0], bad, valid[1]}
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(3, 3, hits), nil
	}
	h.api.download = func(_ context.Context, id string) (doffin.Notice, error) {
		if id == bad {
			return doffin.Notice{}, doffin.ErrInvalidParams
		}
		return notice(id, []byte("ok "+id)), nil
	}
	d := day(2026, 10, 7)
	sum := h.run(t, h.engine(t, ModeSync), d, d)

	var stored []string
	for _, n := range h.store.notices {
		stored = append(stored, n.ID)
	}
	rec := h.finishedDay(t, d)
	switch {
	case len(h.store.attempts) != 0:
		t.Errorf("attempts = %+v, want none: the id was refused before any call", h.store.attempts)
	case !slices.Equal(stored, valid):
		t.Errorf("stored = %v, want the two valid ids %v", stored, valid)
	case rec.State != StatePartial || rec.ErrorCode != CodeItemRejected:
		t.Errorf("day = %s/%s, want partial/item_rejected", rec.State, rec.ErrorCode)
	case rec.Unresolved != 1 || rec.Stored != 2:
		t.Errorf("unresolved = %d stored = %d, want 1 and 2", rec.Unresolved, rec.Stored)
	case sum.Reason != ReasonPartial:
		t.Errorf("run reason = %s, want partial", sum.Reason)
	}
}

// TestSixMalformedIDsDoNotStopTheRun (D-A, case b): six malformed ids in a row do
// not trip the consecutive limit, so the valid id after them is stored and the
// next day is searched.
func TestSixMalformedIDsDoNotStopTheRun(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d1, d2 := day(2026, 10, 6), day(2026, 10, 7)
	valid := ids(1, 2)
	var bad []string
	for i := 1; i <= 6; i++ {
		bad = append(bad, malformedID(i))
	}
	hits := append(slices.Clone(bad), valid[0])
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if p.IssueDateFrom.Equal(d1) {
			return searchResp(7, 7, hits), nil
		}
		return searchResp(1, 1, valid[1:]), nil
	}
	h.api.download = func(_ context.Context, id string) (doffin.Notice, error) {
		if slices.Contains(bad, id) {
			return doffin.Notice{}, doffin.ErrInvalidParams
		}
		return notice(id, []byte("ok "+id)), nil
	}
	sum := h.run(t, h.engine(t, ModeSync), d1, d2)

	var stored []string
	for _, n := range h.store.notices {
		stored = append(stored, n.ID)
	}
	rec1 := h.finishedDay(t, d1)
	switch {
	case sum.Reason == ReasonFailed:
		t.Errorf("run reason = %s, want partial: refused ids must not count as upstream errors", sum.Reason)
	case len(h.api.searchDays()) != 4:
		t.Errorf("searches = %d, want 4: two passes on each of the two days", len(h.api.searchDays()))
	case !slices.Equal(stored, valid):
		t.Errorf("stored = %v, want %v", stored, valid)
	case rec1.State != StatePartial || rec1.ErrorCode != CodeItemRejected || rec1.Unresolved != 6:
		t.Errorf("day 1 = %s/%s unresolved %d, want partial/item_rejected with 6 unresolved", rec1.State, rec1.ErrorCode, rec1.Unresolved)
	case h.finishedDay(t, d2).State != StateDone:
		t.Errorf("day 2 = %s, want done", h.finishedDay(t, d2).State)
	}
}

// TestMalformedIDDoesNotAdvanceConsecutiveCount (D-A, case c): four upstream
// errors, a malformed id, a valid id, then four more upstream errors. The
// malformed id neither counts nor resets, and the valid id resets the count, so
// the run is never stopped.
func TestMalformedIDDoesNotAdvanceConsecutiveCount(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d := day(2026, 10, 7)
	failing := ids(1, 8)
	valid := ids(9, 1)
	bad := malformedID(1)
	hits := slices.Concat(failing[:4], []string{bad, valid[0]}, failing[4:])
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(int64(len(hits)), int64(len(hits)), hits), nil
	}
	h.api.download = func(_ context.Context, id string) (doffin.Notice, error) {
		switch {
		case id == bad:
			return doffin.Notice{}, doffin.ErrInvalidParams
		case slices.Contains(failing, id):
			return doffin.Notice{}, upstream503()
		}
		return notice(id, []byte("ok "+id)), nil
	}
	sum := h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	switch {
	case sum.Reason != ReasonPartial:
		t.Errorf("run reason = %s, want partial: the malformed id must not advance the count", sum.Reason)
	case len(h.api.downloads) != len(hits)-1:
		t.Errorf("downloads = %d, want %d: the malformed id is never sent and the run must not stop early", len(h.api.downloads), len(hits)-1)
	case rec.ErrorCode == CodeConsecutiveErrors:
		t.Errorf("day code = %s, want no consecutive stop", rec.ErrorCode)
	case len(h.store.attempts) != len(failing):
		t.Errorf("attempts = %d, want %d http_error rows and no invalid_id row", len(h.store.attempts), len(failing))
	}
	for _, a := range h.store.attempts {
		if a.Outcome != OutcomeHTTPError {
			t.Errorf("attempt outcome = %s, want %s", a.Outcome, OutcomeHTTPError)
		}
	}
}

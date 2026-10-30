package doffinsync

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"testing"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// T20
func TestHeldIDsAreNeverDownloaded(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 3)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(3, 3, all), nil
	}
	h.store.resolve = map[string]Resolution{all[0]: {Kind: ResolutionHeld}}
	d := day(2026, 10, 7)
	sum := h.run(t, h.engine(t, ModeSync), d, d)

	if want := []string{all[1], all[2]}; len(h.api.downloads) != 2 || h.api.downloads[0] != want[0] || h.api.downloads[1] != want[1] {
		t.Errorf("downloads = %v, want %v", h.api.downloads, want)
	}
	if rec := h.finishedDay(t, d); rec.Held != 1 || rec.State != StateDone {
		t.Errorf("day held = %d state = %s, want held 1 and done", rec.Held, rec.State)
	}
	if sum.Counters.HeldSkipped != 1 {
		t.Errorf("HeldSkipped = %d, want 1", sum.Counters.HeldSkipped)
	}
}

// T22
func TestBodyPassesThroughUnchanged(t *testing.T) {
	t.Parallel()

	h := newHarness()
	id := ids(1, 1)[0]
	body := []byte("\x00\xffnotice\r\n\x00tail")
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(1, 1, []string{id}), nil
	}
	h.api.download = func(_ context.Context, id string) (doffin.Notice, error) {
		return notice(id, body), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	if len(h.store.notices) != 1 {
		t.Fatalf("StoreNotice calls = %d, want 1", len(h.store.notices))
	}
	got := h.store.notices[0]
	sum := sha256.Sum256(body)
	switch {
	case !bytes.Equal(got.Body, body):
		t.Errorf("body reached StoreNotice as %q, want %q", got.Body, body)
	case got.SHA256 != hex.EncodeToString(sum[:]):
		t.Errorf("SHA256 = %s, want the digest of the exact bytes", got.SHA256)
	}
}

// T23
func TestNotFoundYoungDayStaysUnresolved(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 1)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(1, 1, all), nil
	}
	h.api.download = func(context.Context, string) (doffin.Notice, error) {
		return doffin.Notice{}, doffin.ErrNotFound
	}
	d := day(2026, 10, 7) // two days old: younger than NotFoundGrace
	sum := h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	switch {
	case rec.State != StatePartial || rec.ErrorCode != CodeNotFoundGrace:
		t.Errorf("state = %s/%s, want partial/not_found_grace", rec.State, rec.ErrorCode)
	case rec.Unresolved != 1 || rec.NotFound != 0:
		t.Errorf("unresolved = %d not_found = %d, want 1 and 0", rec.Unresolved, rec.NotFound)
	case sum.Counters.NotFound != 0:
		t.Errorf("run NotFound = %d, want 0 while the 404 is unresolved", sum.Counters.NotFound)
	case len(h.store.attempts) != 1 || h.store.attempts[0].Outcome != OutcomeNotFound:
		t.Errorf("attempts = %+v, want one not_found row", h.store.attempts)
	}
}

// T24
func TestNotFoundOldDayIsTerminal(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 1)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(1, 1, all), nil
	}
	h.api.download = func(context.Context, string) (doffin.Notice, error) {
		return doffin.Notice{}, doffin.ErrNotFound
	}
	d := day(2026, 9, 9) // thirty days old
	sum := h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	switch {
	case rec.State != StateDone || rec.ErrorCode != "":
		t.Errorf("state = %s/%s, want done", rec.State, rec.ErrorCode)
	case rec.NotFound != 1 || rec.Held+rec.Stored+rec.NotFound != 1:
		t.Errorf("not_found = %d, held+stored+not_found must equal 1", rec.NotFound)
	case sum.Counters.NotFound != 1:
		t.Errorf("run NotFound = %d, want 1", sum.Counters.NotFound)
	case len(h.store.attempts) != 1 || h.store.attempts[0].Outcome != OutcomeNotFound:
		t.Errorf("attempts = %+v, want one not_found row", h.store.attempts)
	}
}

// T25
func TestItemErrorsQuarantineAndContinue(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name    string
		err     error
		outcome string
		status  int
	}{
		{"empty body", doffin.ErrEmptyResponse, OutcomeEmptyBody, 0},
		{"too large", doffin.ErrResponseTooLarge, OutcomeTooLarge, 0},
		// D-A: an empty outcome means no attempt row; the client refuses the id before any call.
		{"invalid id", doffin.ErrInvalidParams, "", 0},
		{"http 500", &doffin.HTTPError{Operation: "download", Status: 500, Message: "upstream down"}, OutcomeHTTPError, 500},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			h := newHarness()
			all := ids(1, 2)
			h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
				return searchResp(2, 2, all), nil
			}
			h.api.download = func(_ context.Context, id string) (doffin.Notice, error) {
				if id == all[0] {
					return doffin.Notice{}, tc.err
				}
				return notice(id, []byte("ok "+id)), nil
			}
			d := day(2026, 10, 7)
			h.run(t, h.engine(t, ModeSync), d, d)

			if len(h.api.downloads) != 2 || h.api.downloads[1] != all[1] {
				t.Errorf("downloads = %v, want both ids in order", h.api.downloads)
			}
			if len(h.store.notices) != 1 || h.store.notices[0].ID != all[1] {
				t.Errorf("stored = %d, want the second notice only", len(h.store.notices))
			}
			rec := h.finishedDay(t, d)
			if rec.State != StatePartial || rec.ErrorCode != CodeItemRejected || rec.Unresolved != 1 {
				t.Errorf("day = %s/%s unresolved %d, want partial/item_rejected with 1 unresolved", rec.State, rec.ErrorCode, rec.Unresolved)
			}
			if tc.outcome == "" {
				// D-A: a refused id has no attempt row.
				if len(h.store.attempts) != 0 {
					t.Errorf("attempts = %+v, want none: the id was refused before any call", h.store.attempts)
				}
			} else if len(h.store.attempts) != 1 || h.store.attempts[0].Outcome != tc.outcome || h.store.attempts[0].HTTPStatus != tc.status {
				t.Errorf("attempts = %+v, want one %s with status %d", h.store.attempts, tc.outcome, tc.status)
			}
		})
	}
}

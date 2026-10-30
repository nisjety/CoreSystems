package doffinsync

import (
	"context"
	"slices"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// The bounded retry of notices refused for their content (R1 to R4). The store
// reports facts; these tests pin the policy the engine applies to them with its own
// clock: a rejection under RejectCooloff old is cooling, an older one is retried,
// and the MaxRejections-th is terminal and counts in its own bucket.

// rejectionDay is the day these tests sync: closed, and younger than NotFoundGrace.
var rejectionDay = day(2026, 10, 7)

// refused is what the store reports for an id that has been refused n times for its
// content, the latest refusal ago before the engine's clock (testNow). A negative
// ago is a refusal stamped in the future.
func refused(n int, ago time.Duration) Resolution {
	return Resolution{Kind: ResolutionRejected, Rejections: n, LastRejection: testNow.Add(-ago)}
}

// searchAll answers every search with one complete page that holds all.
func (h *harness) searchAll(all []string) {
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(int64(len(all)), int64(len(all)), all), nil
	}
}

// downloadFails makes Download fail with the error scripted for an id and return the
// notice for any other id.
func (h *harness) downloadFails(errs map[string]error) {
	h.api.download = func(_ context.Context, id string) (doffin.Notice, error) {
		if err, ok := errs[id]; ok {
			return doffin.Notice{}, err
		}
		return notice(id, []byte("ok "+id)), nil
	}
}

// storedIDs lists the ids StoreNotice saw, in order.
func storedIDs(h *harness) []string {
	var out []string
	for _, n := range h.store.notices {
		out = append(out, n.ID)
	}
	return out
}

// TestRejectionPolicyConstants pins the decided policy: a refused notice rests for 24
// hours, and its third refusal is terminal. The other tests use the constants by name,
// so only this one fails when a value changes.
func TestRejectionPolicyConstants(t *testing.T) {
	t.Parallel()

	if RejectCooloff != 24*time.Hour {
		t.Errorf("RejectCooloff = %v, want 24h", RejectCooloff)
	}
	if MaxRejections != 3 {
		t.Errorf("MaxRejections = %d, want 3", MaxRejections)
	}
}

// TestCoolingRejectionIsNotDownloaded (R1): an id with fewer than MaxRejections
// refusals whose latest is under RejectCooloff old is cooling. It is unresolved
// with item_rejected, it is not downloaded, it makes no attempt row and it spends
// no budget. The rest of the day is still processed.
func TestCoolingRejectionIsNotDownloaded(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name string
		res  Resolution
	}{
		{"refused an hour ago", refused(1, time.Hour)},
		{"second refusal a minute ago", refused(2, time.Minute)},
		{"one nanosecond inside the cool-off", refused(1, RejectCooloff-time.Nanosecond)},
		{"refused at this very instant", refused(1, 0)},
		{"stamped in the future", refused(1, -time.Hour)},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			h := newHarness()
			all := ids(1, 2)
			h.searchAll(all)
			h.store.resolve = map[string]Resolution{all[0]: tc.res}
			sum := h.run(t, h.engine(t, ModeSync), rejectionDay, rejectionDay)

			rec := h.finishedDay(t, rejectionDay)
			switch {
			case !slices.Equal(h.api.downloads, []string{all[1]}):
				t.Errorf("downloads = %v, want only %v: a cooling id must not be sent", h.api.downloads, all[1])
			case len(h.store.attempts) != 0:
				t.Errorf("attempts = %+v, want none for a cooling id", h.store.attempts)
			case rec.State != StatePartial || rec.ErrorCode != CodeItemRejected:
				t.Errorf("day = %s/%s, want partial/item_rejected", rec.State, rec.ErrorCode)
			case rec.Unresolved != 1 || rec.Rejected != 0 || rec.Stored != 1:
				t.Errorf("unresolved %d rejected %d stored %d, want 1, 0 and 1: a cooling id is unresolved, not rejected",
					rec.Unresolved, rec.Rejected, rec.Stored)
			case sum.Counters.Downloads != 1:
				t.Errorf("Downloads = %d, want 1: a cooling id spends no budget", sum.Counters.Downloads)
			}
		})
	}
}

// TestRetryEligibleRejectionIsDownloadedAgain (R1): an id with fewer than
// MaxRejections refusals whose latest is RejectCooloff old or older is an ordinary
// download candidate. Stored, it leaves the day done.
func TestRetryEligibleRejectionIsDownloadedAgain(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name string
		res  Resolution
	}{
		{"exactly at the cool-off", refused(1, RejectCooloff)},
		{"a day and an hour ago", refused(1, RejectCooloff+time.Hour)},
		{"second refusal two days ago", refused(2, 48*time.Hour)},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			h := newHarness()
			all := ids(1, 2)
			h.searchAll(all)
			h.store.resolve = map[string]Resolution{all[0]: tc.res}
			sum := h.run(t, h.engine(t, ModeSync), rejectionDay, rejectionDay)

			rec := h.finishedDay(t, rejectionDay)
			switch {
			case !slices.Equal(h.api.downloads, all):
				t.Errorf("downloads = %v, want %v: an eligible id is downloaded again", h.api.downloads, all)
			case !slices.Equal(storedIDs(h), all):
				t.Errorf("stored = %v, want %v", storedIDs(h), all)
			case rec.State != StateDone || rec.ErrorCode != "":
				t.Errorf("day = %s/%s, want done", rec.State, rec.ErrorCode)
			case rec.Stored != 2 || rec.Unresolved != 0 || rec.Rejected != 0:
				t.Errorf("stored %d unresolved %d rejected %d, want 2, 0 and 0", rec.Stored, rec.Unresolved, rec.Rejected)
			case sum.Counters.Downloads != 2:
				t.Errorf("Downloads = %d, want 2", sum.Counters.Downloads)
			}
		})
	}
}

// TestTerminalRejectionFromHistoryIsCountedAndTheDayIsDone (R1, R4): an id that has
// been refused MaxRejections times or more is terminal, however recent its latest
// refusal. It is never downloaded, it is counted as rejected and not as unresolved,
// and the day can be done because held + stored + not_found + rejected add up to
// the accessible count.
func TestTerminalRejectionFromHistoryIsCountedAndTheDayIsDone(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 5)
	h.searchAll(all)
	h.store.resolve = map[string]Resolution{
		all[0]: refused(MaxRejections, 25*time.Hour),
		all[1]: refused(MaxRejections, time.Minute), // terminal although the refusal is recent
		all[2]: refused(MaxRejections+4, 10*24*time.Hour),
	}
	sum := h.run(t, h.engine(t, ModeSync), rejectionDay, rejectionDay)

	rec := h.finishedDay(t, rejectionDay)
	accounted := int64(rec.Held + rec.Stored + rec.NotFound + rec.Rejected)
	switch {
	case !slices.Equal(h.api.downloads, []string{all[3], all[4]}):
		t.Errorf("downloads = %v, want %v: a terminal id is never sent", h.api.downloads, []string{all[3], all[4]})
	case len(h.store.attempts) != 0:
		t.Errorf("attempts = %+v, want none: a terminal id makes no call", h.store.attempts)
	case rec.State != StateDone || rec.ErrorCode != "":
		t.Errorf("day = %s/%s, want done: terminal ids are resolved", rec.State, rec.ErrorCode)
	case rec.Rejected != 3 || rec.Stored != 2 || rec.Unresolved != 0:
		t.Errorf("rejected %d stored %d unresolved %d, want 3, 2 and 0", rec.Rejected, rec.Stored, rec.Unresolved)
	case rec.Accessible == nil || accounted != *rec.Accessible:
		t.Errorf("held+stored+not_found+rejected = %d, want the accessible count %v", accounted, rec.Accessible)
	case sum.Reason != ReasonDone:
		t.Errorf("run reason = %s, want done", sum.Reason)
	case len(sum.Days) != 1 || sum.Days[0].Rejected != 3:
		t.Errorf("day outcomes = %+v, want one with Rejected 3", sum.Days)
	case sum.Counters.Downloads != 2:
		t.Errorf("Downloads = %d, want 2", sum.Counters.Downloads)
	}
}

// TestThirdRefusalInThisRunIsTerminal (R1, R4): a download refused right now that
// makes the total MaxRejections is terminal at once, in the same run. The attempt is
// recorded, the id counts as rejected for the day and not as unresolved, and the day
// is done.
func TestThirdRefusalInThisRunIsTerminal(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name    string
		err     error
		outcome string
	}{
		{"too large", doffin.ErrResponseTooLarge, OutcomeTooLarge},
		{"empty body", doffin.ErrEmptyResponse, OutcomeEmptyBody},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			h := newHarness()
			all := ids(1, 2)
			h.searchAll(all)
			h.store.resolve = map[string]Resolution{all[0]: refused(MaxRejections-1, 30*time.Hour)}
			h.downloadFails(map[string]error{all[0]: tc.err})
			sum := h.run(t, h.engine(t, ModeSync), rejectionDay, rejectionDay)

			rec := h.finishedDay(t, rejectionDay)
			switch {
			case !slices.Equal(h.api.downloads, all):
				t.Errorf("downloads = %v, want %v", h.api.downloads, all)
			case len(h.store.attempts) != 1 || h.store.attempts[0].DoffinID != all[0] || h.store.attempts[0].Outcome != tc.outcome:
				t.Errorf("attempts = %+v, want one %s row for %s", h.store.attempts, tc.outcome, all[0])
			case rec.State != StateDone || rec.ErrorCode != "":
				t.Errorf("day = %s/%s, want done: the third refusal is terminal in this run", rec.State, rec.ErrorCode)
			case rec.Rejected != 1 || rec.Unresolved != 0 || rec.Stored != 1:
				t.Errorf("rejected %d unresolved %d stored %d, want 1, 0 and 1", rec.Rejected, rec.Unresolved, rec.Stored)
			case sum.Reason != ReasonDone:
				t.Errorf("run reason = %s, want done", sum.Reason)
			case len(sum.Days) != 1 || sum.Days[0].Rejected != 1:
				t.Errorf("day outcomes = %+v, want one with Rejected 1", sum.Days)
			}
		})
	}
}

// TestRefusalThatIsNotTheThirdStaysUnresolved (R1): a download refused now that
// makes the total one or two is recorded and leaves the id unresolved with
// item_rejected, so the day stays partial. It is not terminal yet.
func TestRefusalThatIsNotTheThirdStaysUnresolved(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name    string
		history *Resolution // what the store knows of the id before the download; nil for none
		err     error
		outcome string
	}{
		{"first refusal, too large", nil, doffin.ErrResponseTooLarge, OutcomeTooLarge},
		{"first refusal, empty body", nil, doffin.ErrEmptyResponse, OutcomeEmptyBody},
		{"second refusal, too large", ptr(refused(1, 25*time.Hour)), doffin.ErrResponseTooLarge, OutcomeTooLarge},
		{"second refusal, empty body", ptr(refused(1, 25*time.Hour)), doffin.ErrEmptyResponse, OutcomeEmptyBody},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			h := newHarness()
			all := ids(1, 2)
			h.searchAll(all)
			if tc.history != nil {
				h.store.resolve = map[string]Resolution{all[0]: *tc.history}
			}
			h.downloadFails(map[string]error{all[0]: tc.err})
			sum := h.run(t, h.engine(t, ModeSync), rejectionDay, rejectionDay)

			rec := h.finishedDay(t, rejectionDay)
			switch {
			case len(h.store.attempts) != 1 || h.store.attempts[0].DoffinID != all[0] || h.store.attempts[0].Outcome != tc.outcome:
				t.Errorf("attempts = %+v, want one %s row for %s", h.store.attempts, tc.outcome, all[0])
			case rec.State != StatePartial || rec.ErrorCode != CodeItemRejected:
				t.Errorf("day = %s/%s, want partial/item_rejected", rec.State, rec.ErrorCode)
			case rec.Unresolved != 1 || rec.Rejected != 0 || rec.Stored != 1:
				t.Errorf("unresolved %d rejected %d stored %d, want 1, 0 and 1: not terminal yet", rec.Unresolved, rec.Rejected, rec.Stored)
			case sum.Reason != ReasonPartial:
				t.Errorf("run reason = %s, want partial", sum.Reason)
			}
		})
	}
}

// ptr returns a pointer to a copy of v.
func ptr[T any](v T) *T { return &v }

// TestRejectionsDoNotTouchTheConsecutiveCount (R1, D-B1): a cooling id, a terminal
// id from history, a refusal that makes the third and a refusal that does not are
// neither counted toward the five in a row nor reset by it. Four counted errors,
// all of those, then a fifth counted error stop the day: a reset would let the
// fifth through, and a count would stop the day before it.
func TestRejectionsDoNotTouchTheConsecutiveCount(t *testing.T) {
	t.Parallel()

	h := newHarness()
	failing := ids(1, 4)
	cooling := ids(5, 2)
	terminal := ids(7, 1)
	thirdNow := ids(8, 1)
	firstNow := ids(9, 1)
	fifth := ids(10, 1)
	hits := slices.Concat(failing, cooling, terminal, thirdNow, firstNow, fifth)
	h.searchAll(hits)
	h.store.resolve = map[string]Resolution{
		cooling[0]:  refused(1, time.Hour),
		cooling[1]:  refused(2, 2*time.Hour),
		terminal[0]: refused(MaxRejections, 48*time.Hour),
		thirdNow[0]: refused(MaxRejections-1, 48*time.Hour),
	}
	script := map[string]error{
		thirdNow[0]: doffin.ErrResponseTooLarge,
		firstNow[0]: doffin.ErrEmptyResponse,
	}
	for _, id := range slices.Concat(failing, fifth) {
		script[id] = &doffin.HTTPError{Operation: "download", Status: 503, Message: "busy"}
	}
	h.downloadFails(script)
	sum := h.run(t, h.engine(t, ModeSync), rejectionDay, rejectionDay)

	wantDownloads := slices.Concat(failing, thirdNow, firstNow, fifth)
	rec := h.finishedDay(t, rejectionDay)
	switch {
	case !slices.Equal(h.api.downloads, wantDownloads):
		t.Errorf("downloads = %v, want %v: the rejections must neither stop the day early nor be skipped", h.api.downloads, wantDownloads)
	case rec.State != StateFailed || rec.ErrorCode != CodeConsecutiveErrors:
		t.Errorf("day = %s/%s, want failed/consecutive_errors on the fifth counted error: a rejection reset the count", rec.State, rec.ErrorCode)
	case sum.Reason != ReasonFailed:
		t.Errorf("run reason = %s, want failed", sum.Reason)
	}
}

// TestManyTerminalRefusalsDoNotStopTheRun (R1, D-B1): ten ids each refused for the
// third time in one day are ten rejections, not ten upstream errors. All are
// attempted, the day is done, and the run is done.
func TestManyTerminalRefusalsDoNotStopTheRun(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 10)
	h.searchAll(all)
	h.store.resolve = make(map[string]Resolution, len(all))
	script := make(map[string]error, len(all))
	for i, id := range all {
		h.store.resolve[id] = refused(MaxRejections-1, 30*time.Hour)
		if i%2 == 0 {
			script[id] = doffin.ErrResponseTooLarge
		} else {
			script[id] = doffin.ErrEmptyResponse
		}
	}
	h.downloadFails(script)
	sum := h.run(t, h.engine(t, ModeSync), rejectionDay, rejectionDay)

	rec := h.finishedDay(t, rejectionDay)
	switch {
	case len(h.api.downloads) != 10:
		t.Errorf("downloads = %d, want all 10: refusals must not stop the day", len(h.api.downloads))
	case len(h.store.attempts) != 10:
		t.Errorf("attempts = %d, want 10", len(h.store.attempts))
	case rec.State != StateDone || rec.Rejected != 10 || rec.Unresolved != 0:
		t.Errorf("day = %s rejected %d unresolved %d, want done with 10 rejected", rec.State, rec.Rejected, rec.Unresolved)
	case sum.Reason != ReasonDone:
		t.Errorf("run reason = %s, want done", sum.Reason)
	}
}

// TestRejectionPolicyUsesTheEngineClock (R1): the cool-off is judged with
// Options.Now, not with the wall clock. The engine's clock here is years ahead of
// the wall clock; an engine that read the wall clock would see both stamps as
// future refusals and hold the eligible id back.
func TestRejectionPolicyUsesTheEngineClock(t *testing.T) {
	t.Parallel()

	clock := time.Date(2030, time.June, 15, 12, 0, 0, 0, time.UTC)
	d := day(2030, 6, 10)
	h := newHarness()
	all := ids(1, 2)
	h.searchAll(all)
	h.store.resolve = map[string]Resolution{
		all[0]: {Kind: ResolutionRejected, Rejections: 1, LastRejection: clock.Add(-time.Hour)},
		all[1]: {Kind: ResolutionRejected, Rejections: 1, LastRejection: clock.Add(-25 * time.Hour)},
	}
	e := h.engine(t, ModeSync, func(o *Options) { o.Now = func() time.Time { return clock } })
	h.run(t, e, d, d)

	rec := h.finishedDay(t, d)
	switch {
	case !slices.Equal(h.api.downloads, []string{all[1]}):
		t.Errorf("downloads = %v, want only %v: the id refused an hour before the engine's clock is cooling and the other is eligible",
			h.api.downloads, all[1])
	case rec.State != StatePartial || rec.Unresolved != 1 || rec.Stored != 1:
		t.Errorf("day = %s unresolved %d stored %d, want partial with 1 and 1", rec.State, rec.Unresolved, rec.Stored)
	}
}

// TestDayAccountingAddsUpAcrossEveryBucket (R4): a day with a held id, a stored id,
// an old-day 404, a terminal rejection and a refusal that becomes terminal in this
// run is done when held + stored + not_found + rejected equal the accessible count.
func TestDayAccountingAddsUpAcrossEveryBucket(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d := day(2026, 9, 9) // thirty days old: a 404 is terminal
	all := ids(1, 5)
	h.searchAll(all)
	h.store.resolve = map[string]Resolution{
		all[0]: {Kind: ResolutionHeld},
		all[1]: {Kind: ResolutionNotFound},
		all[2]: refused(MaxRejections, 5*24*time.Hour),
		all[3]: refused(MaxRejections-1, 3*24*time.Hour),
	}
	h.downloadFails(map[string]error{all[3]: doffin.ErrEmptyResponse})
	sum := h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	switch {
	case rec.State != StateDone || rec.ErrorCode != "":
		t.Errorf("day = %s/%s, want done", rec.State, rec.ErrorCode)
	case rec.Held != 1 || rec.NotFound != 1 || rec.Rejected != 2 || rec.Stored != 1 || rec.Unresolved != 0:
		t.Errorf("held %d not_found %d rejected %d stored %d unresolved %d, want 1, 1, 2, 1 and 0",
			rec.Held, rec.NotFound, rec.Rejected, rec.Stored, rec.Unresolved)
	case rec.Accessible == nil || int64(rec.Held+rec.Stored+rec.NotFound+rec.Rejected) != *rec.Accessible:
		t.Errorf("buckets do not add up to the accessible count %v", rec.Accessible)
	case len(sum.Days) != 1 || sum.Days[0].Rejected != 2:
		t.Errorf("day outcomes = %+v, want one with Rejected 2", sum.Days)
	}
}

package doffinsync

import (
	"slices"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// TestRejectedIDIsNotDownloadedWhileCooling (E3, R1): an id the store reports as
// rejected, with fewer than MaxRejections refusals and its latest one under
// RejectCooloff old, is cooling. It is unresolved. It is not downloaded, no
// attempt is recorded and no budget is spent. The rest of the day is still
// processed. (Before R1 an id refused once was never downloaded again, D-B2.)
func TestRejectedIDIsNotDownloadedWhileCooling(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 3)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(3, 3, all), nil
	}
	h.store.resolve = map[string]Resolution{
		all[0]: {Kind: ResolutionRejected, Rejections: 1, LastRejection: testNow.Add(-time.Hour)},
	}
	d := day(2026, 10, 7)
	sum := h.run(t, h.engine(t, ModeSync), d, d)

	var stored []string
	for _, n := range h.store.notices {
		stored = append(stored, n.ID)
	}
	rec := h.finishedDay(t, d)
	switch {
	case !slices.Equal(h.api.downloads, []string{all[1], all[2]}):
		t.Errorf("downloads = %v, want %v: a cooling id must not be sent", h.api.downloads, []string{all[1], all[2]})
	case len(h.store.attempts) != 0:
		t.Errorf("attempts = %+v, want none for a cooling id", h.store.attempts)
	case !slices.Equal(stored, []string{all[1], all[2]}):
		t.Errorf("stored = %v, want the two other ids", stored)
	case rec.State != StatePartial || rec.ErrorCode != CodeItemRejected || rec.Unresolved != 1 || rec.Stored != 2:
		t.Errorf("day = %s/%s unresolved %d stored %d, want partial/item_rejected with 1 unresolved and 2 stored",
			rec.State, rec.ErrorCode, rec.Unresolved, rec.Stored)
	case sum.Counters.Downloads != 2:
		t.Errorf("Downloads = %d, want 2: a cooling id spends no budget", sum.Counters.Downloads)
	case sum.Reason != ReasonPartial:
		t.Errorf("run reason = %s, want partial", sum.Reason)
	}
}

package doffinsync

import (
	"slices"
	"testing"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// TestMalformedIDIsNeverSentAndSpendsNoBudget (E6): a malformed id is refused
// before any call. Download is never invoked for it, no Downloads counter moves
// and no attempt is recorded. With MaxDownloads 2, two malformed ids in front of
// two valid ones must not stop the day at the limit.
func TestMalformedIDIsNeverSentAndSpendsNoBudget(t *testing.T) {
	t.Parallel()

	h := newHarness()
	valid := ids(1, 2)
	hits := []string{malformedID(1), malformedID(2), valid[0], valid[1]}
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(4, 4, hits), nil
	}
	d := day(2026, 10, 7)
	sum := h.run(t, h.engine(t, ModeSync, func(o *Options) { o.MaxDownloads = 2 }), d, d)

	rec := h.finishedDay(t, d)
	switch {
	case !slices.Equal(h.api.downloads, valid):
		t.Errorf("downloads = %v, want only the valid ids %v", h.api.downloads, valid)
	case len(h.store.attempts) != 0:
		t.Errorf("attempts = %+v, want none for malformed ids", h.store.attempts)
	case sum.Counters.Downloads != 2:
		t.Errorf("Downloads = %d, want 2", sum.Counters.Downloads)
	case rec.ErrorCode == CodeDownloadLimit:
		t.Errorf("day code = download_limit, want none: malformed ids must not consume the limit")
	case rec.State != StatePartial || rec.ErrorCode != CodeItemRejected || rec.Unresolved != 2:
		t.Errorf("day = %s/%s unresolved %d, want partial/item_rejected with 2 unresolved", rec.State, rec.ErrorCode, rec.Unresolved)
	}
}

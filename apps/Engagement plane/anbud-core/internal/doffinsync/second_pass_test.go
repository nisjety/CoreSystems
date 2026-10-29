package doffinsync

import (
	"slices"
	"testing"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// TestSecondPassMembershipChangeIsCountChanged (E4, D-B3): the day is paged
// twice. On the second pass one member leaves and one non-member arrives, and
// the total stays the same. The passes disagree, so the day is partial with
// count_changed and nothing is downloaded.
func TestSecondPassMembershipChangeIsCountChanged(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 150)
	swapped := slices.Clone(all)
	swapped[149] = "2026-900001" // the last member leaves, a non-member arrives
	passes := 0
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if pageNum(p) == 1 {
			passes++
		}
		list := all
		if passes == 2 {
			list = swapped
		}
		return searchResp(150, 150, pageOf(list, pageNum(p))), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	switch {
	case rec.State != StatePartial || rec.ErrorCode != CodeCountChanged:
		t.Errorf("day = %s/%s, want partial/count_changed", rec.State, rec.ErrorCode)
	case len(h.api.downloads) != 0:
		t.Errorf("downloads = %d, want none: the passes disagree", len(h.api.downloads))
	}
}

// TestTwoAgreeingPassesCompleteTheDay (E4, D-B3): two passes that agree complete
// the day. Every page of both passes is requested, in order.
func TestTwoAgreeingPassesCompleteTheDay(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 150)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(150, 150, pageOf(all, pageNum(p))), nil
	}
	h.store.resolve = held(all)
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	if got := h.api.pageNumbers(); !slices.Equal(got, []int{1, 2, 1, 2}) {
		t.Errorf("pages = %v, want [1 2 1 2]: two full passes", got)
	}
	if rec := h.finishedDay(t, d); rec.State != StateDone {
		t.Errorf("state = %s/%s, want done when both passes agree", rec.State, rec.ErrorCode)
	}
}

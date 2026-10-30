package doffinsync

import (
	"slices"
	"testing"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// T13
func TestSearchIsOneDayPinnedAndSorted(t *testing.T) {
	t.Parallel()

	h := newHarness()
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(0, 0, nil), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	if len(h.api.searches) != 2 {
		t.Fatalf("searches = %d, want 2: two passes over the day (D-B3)", len(h.api.searches))
	}
	c := h.api.searches[0]
	switch {
	case !c.From.Equal(d) || !c.To.Equal(d):
		t.Errorf("search window = [%v, %v], want the single day %v", c.From, c.To, d)
	case c.Size != 100:
		t.Errorf("NumHitsPerPage = %d, want 100", c.Size)
	case c.Sort != "PUBLICATION_DATE_ASC":
		t.Errorf("SortBy = %q, want PUBLICATION_DATE_ASC", c.Sort)
	case !c.PageSet || c.Page != 1:
		t.Errorf("Page = %d (set %v), want 1", c.Page, c.PageSet)
	}
}

// T14
func TestPagingRequestsPagesOneThroughN(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 250)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(250, 250, pageOf(all, pageNum(p))), nil
	}
	h.store.resolve = held(all)
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	if got := h.api.pageNumbers(); !slices.Equal(got, []int{1, 2, 3, 1, 2, 3}) {
		t.Fatalf("pages = %v, want [1 2 3 1 2 3]: two passes (D-B3)", got)
	}
	if rec := h.finishedDay(t, d); rec.State != StateDone {
		t.Errorf("state = %s, want done", rec.State)
	}
}

// T15
func TestSaturatedDayHasNoDownloadsAndNoRows(t *testing.T) {
	t.Parallel()

	h := newHarness()
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(1001, 1000, ids(1, 100)), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	switch {
	case rec.State != StateCapped || rec.ErrorCode != CodeSaturated:
		t.Errorf("state = %s/%s, want capped/saturated", rec.State, rec.ErrorCode)
	case len(h.api.downloads) != 0:
		t.Errorf("downloads = %v, want none", h.api.downloads)
	case len(h.store.notices) != 0:
		t.Errorf("stored %d notices, want none", len(h.store.notices))
	case len(h.api.searches) != 1:
		t.Errorf("searches = %d, want 1 (no page beyond 1)", len(h.api.searches))
	}
}

// T16
func TestCapBoundaryIsNotSaturated(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 1000)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(1000, 1000, pageOf(all, pageNum(p))), nil
	}
	h.store.resolve = held(all)
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	if got := h.api.pageNumbers(); !slices.Equal(got, slices.Concat(seq(1, 10), seq(1, 10))) {
		t.Fatalf("pages = %v, want 1 through 10 twice (D-B3)", got)
	}
	if rec := h.finishedDay(t, d); rec.State != StateDone {
		t.Errorf("state = %s/%s, want done at T == A == 1000", rec.State, rec.ErrorCode)
	}
}

// T17
func TestPageCountChangeIsCountChanged(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 150)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if pageNum(p) == 2 {
			return searchResp(46, 150, pageOf(all, 2)), nil
		}
		return searchResp(150, 150, pageOf(all, 1)), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	if rec.State != StatePartial || rec.ErrorCode != CodeCountChanged {
		t.Errorf("state = %s/%s, want partial/count_changed", rec.State, rec.ErrorCode)
	}
	if len(h.api.downloads) != 0 {
		t.Errorf("downloads = %d, want none", len(h.api.downloads))
	}
}

// T18
func TestUniqueMismatchStopsBeforeDownloads(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 150)
	// Page 2 repeats ten ids from page 1 and adds forty new ones: 140 distinct of 150.
	page2 := append(append([]string{}, all[:10]...), all[100:140]...)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if pageNum(p) == 2 {
			return searchResp(150, 150, page2), nil
		}
		return searchResp(150, 150, all[:100]), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	if rec.State != StatePartial || rec.ErrorCode != CodeCountMismatch {
		t.Errorf("state = %s/%s, want partial/count_mismatch", rec.State, rec.ErrorCode)
	}
	if len(h.api.downloads) != 0 {
		t.Errorf("downloads = %d, want none", len(h.api.downloads))
	}
}

// T19
func TestRepeatAndDropIsCaught(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 150)
	// Page 2 repeats id 1 and omits id 150: 149 distinct of 150.
	page2 := append([]string{all[0]}, all[100:149]...)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if pageNum(p) == 2 {
			return searchResp(150, 150, page2), nil
		}
		return searchResp(150, 150, all[:100]), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	if rec.State != StatePartial || rec.ErrorCode != CodeCountMismatch {
		t.Errorf("state = %s/%s, want partial/count_mismatch", rec.State, rec.ErrorCode)
	}
	if len(h.api.downloads) != 0 {
		t.Errorf("downloads = %d, want none", len(h.api.downloads))
	}
}

// T21
func TestIDsDedupedAcrossPages(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 101)
	// Id 100 appears on page 1 and again on page 2; the day still has 101 distinct ids.
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if pageNum(p) == 2 {
			return searchResp(101, 101, []string{all[99], all[100]}), nil
		}
		return searchResp(101, 101, all[:100]), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	counts := make(map[string]int)
	for _, id := range h.api.downloads {
		counts[id]++
	}
	if len(counts) != 101 {
		t.Fatalf("distinct downloads = %d, want 101", len(counts))
	}
	for id, n := range counts {
		if n != 1 {
			t.Errorf("%s downloaded %d times, want once", id, n)
		}
	}
	if len(h.store.resolveCalls) != 1 || len(h.store.resolveCalls[0]) != 101 {
		t.Errorf("Resolve calls = %v, want one call with 101 ids", h.store.resolveCalls)
	}
}

// T38
func TestZeroHitDayIsDone(t *testing.T) {
	t.Parallel()

	h := newHarness()
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(0, 0, nil), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	switch {
	case rec.State != StateDone || rec.ErrorCode != "":
		t.Errorf("state = %s/%s, want done", rec.State, rec.ErrorCode)
	case len(h.api.downloads) != 0:
		t.Errorf("downloads = %d, want none", len(h.api.downloads))
	case len(h.store.resolveCalls) != 0:
		t.Errorf("Resolve called %d times, want none for a zero-hit day", len(h.store.resolveCalls))
	}
}

// T39
func TestRunIsDoneOnlyWhenEveryDayIsDone(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d1, d2 := day(2026, 10, 6), day(2026, 10, 7)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if p.IssueDateFrom.Equal(d2) {
			return searchResp(1, 2, ids(1, 1)), nil // count_mismatch: T < A
		}
		return searchResp(0, 0, nil), nil
	}
	sum := h.run(t, h.engine(t, ModeSync), d1, d2)

	if got := h.finishedDay(t, d1).State; got != StateDone {
		t.Errorf("first day state = %s, want done", got)
	}
	if rec := h.finishedDay(t, d2); rec.State != StatePartial {
		t.Errorf("second day state = %s/%s, want partial", rec.State, rec.ErrorCode)
	}
	if sum.Reason != ReasonPartial {
		t.Errorf("run reason = %s, want partial", sum.Reason)
	}
}

package doffinsync

import (
	"errors"
	"slices"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// Live test V2 (2026-10-10): the order of tied hits inside one publication day is not
// stable between requests. On a day of 94 hits at 25 per page one sweep returned all 94
// ids, the next returned 65 distinct with 29 repeated, and the union of the two held all
// 94. A day is therefore complete when the union of its sweeps equals the total, not
// when one sweep does (D-B3, revised).

// sweepServer serves each sweep of the day from its own list of hits, paged 100 to a
// page. A sweep starts at each request for page 1. The last list serves every later
// sweep. A list may repeat ids and omit others, the way a torn sweep does.
func sweepServer(h *harness, total int64, lists [][]string) {
	sweep := 0
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if pageNum(p) == 1 {
			sweep++
		}
		list := lists[min(sweep, len(lists))-1]
		return searchResp(total, total, pageOf(list, pageNum(p))), nil
	}
}

// tornSweep is a sweep of ids(1, 150) whose page 2 repeats ten ids of page 1 and misses
// ten others: 140 distinct ids of 150.
func tornSweep() []string {
	all := ids(1, 150)
	return slices.Concat(all[:100], all[90:140])
}

// A torn first sweep does not end the day. The second sweep is complete, and the union
// of the two is the whole day, so the day is done after two sweeps.
func TestUnionOfSweepsCompletesAnUnstableDay(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 150)
	sweepServer(h, 150, [][]string{tornSweep(), all})
	h.store.resolve = held(all)
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	if got := h.api.pageNumbers(); !slices.Equal(got, []int{1, 2, 1, 2}) {
		t.Errorf("pages = %v, want [1 2 1 2]: two sweeps are enough when their union is the day", got)
	}
	rec := h.finishedDay(t, d)
	if rec.State != StateDone || rec.Held != 150 {
		t.Errorf("day = %s/%s held %d, want done with all 150 ids held", rec.State, rec.ErrorCode, rec.Held)
	}
}

// Two torn sweeps in a row leave ten ids unseen. The day is swept again, and the third
// sweep completes the union.
func TestAnUnstableDayIsSweptAgainUntilTheUnionIsTheDay(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 150)
	sweepServer(h, 150, [][]string{tornSweep(), tornSweep(), all})
	h.store.resolve = held(all)
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	if got := h.api.pageNumbers(); !slices.Equal(got, []int{1, 2, 1, 2, 1, 2}) {
		t.Errorf("pages = %v, want three sweeps of pages 1 and 2", got)
	}
	rec := h.finishedDay(t, d)
	if rec.State != StateDone || rec.Held != 150 {
		t.Errorf("day = %s/%s held %d, want done with all 150 ids held", rec.State, rec.ErrorCode, rec.Held)
	}
}

// A day the union never reaches is swept maxSweeps times and then left partial. The
// cost is bounded and nothing is downloaded.
func TestSweepsAreCappedAndTheDayStaysPartial(t *testing.T) {
	t.Parallel()

	h := newHarness()
	sweepServer(h, 150, [][]string{tornSweep()})
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	if got, want := h.api.pageNumbers(), slices.Repeat([]int{1, 2}, maxSweeps); !slices.Equal(got, want) {
		t.Errorf("pages = %v, want %d sweeps of pages 1 and 2", got, maxSweeps)
	}
	rec := h.finishedDay(t, d)
	if rec.State != StatePartial || rec.ErrorCode != CodeCountMismatch {
		t.Errorf("day = %s/%s, want partial/count_mismatch", rec.State, rec.ErrorCode)
	}
	if len(h.api.downloads) != 0 {
		t.Errorf("downloads = %d, want none", len(h.api.downloads))
	}
}

// pageOfSize returns the hits of page n (1-based, size to a page) of all.
func pageOfSize(all []string, n, size int) []string {
	start := (n - 1) * size
	if n < 1 || start >= len(all) {
		return nil
	}
	return all[start:min(start+size, len(all))]
}

// A smaller page size makes a short day span several pages, which is how the real
// paging is tried on a day of fewer than 100 hits. The size is the one asked for in
// every search, and it is part of the label the page is stored under.
func TestPageSizeOptionSetsTheSearchSizeAndTheQueryKey(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 94)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(94, 94, pageOfSize(all, pageNum(p), p.NumHitsPerPage)), nil
	}
	h.store.resolve = held(all)
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync, func(o *Options) { o.PageSize = 25 }), d, d)

	if got := h.api.pageNumbers(); !slices.Equal(got, []int{1, 2, 3, 4, 1, 2, 3, 4}) {
		t.Errorf("pages = %v, want pages 1 to 4 twice: 94 hits at 25 to a page", got)
	}
	for _, c := range h.api.searches {
		if c.Size != 25 {
			t.Errorf("NumHitsPerPage = %d, want 25", c.Size)
		}
	}
	if len(h.store.pages) == 0 {
		t.Fatal("no search page was stored")
	}
	for _, p := range h.store.pages {
		if !strings.Contains(p.QueryKey, "perPage=25;") {
			t.Errorf("query key %q does not carry the page size", p.QueryKey)
		}
	}
	if rec := h.finishedDay(t, d); rec.State != StateDone {
		t.Errorf("day = %s/%s, want done", rec.State, rec.ErrorCode)
	}
}

// A page size below zero or above what Doffin allows is refused when the engine is made.
func TestPageSizeOutsideTheAPILimitsIsRefused(t *testing.T) {
	t.Parallel()

	for _, size := range []int{-1, doffin.MaxHitsPerPage + 1} {
		h := newHarness()
		if _, err := New(h.deps(), Options{Mode: ModeSync, PageSize: size}); !errors.Is(err, ErrInvalidOptions) {
			t.Errorf("New with page size %d: err = %v, want ErrInvalidOptions", size, err)
		}
	}
}

// A day whose only page returns fewer hits than the total cannot be an ordering
// problem: one page has no page boundary. It is a mismatch at once, with one search.
func TestSinglePageShortfallIsNotSweptAgain(t *testing.T) {
	t.Parallel()

	h := newHarness()
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(10, 10, ids(1, 8)), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	if rec.State != StatePartial || rec.ErrorCode != CodeCountMismatch {
		t.Errorf("day = %s/%s, want partial/count_mismatch", rec.State, rec.ErrorCode)
	}
	if got := len(h.api.searches); got != 1 {
		t.Errorf("searches = %d, want 1: a single page is never swept again", got)
	}
	if len(h.api.downloads) != 0 {
		t.Errorf("downloads = %d, want none", len(h.api.downloads))
	}
}

// A perfect sweep followed by a torn one is the shape V2 found live. The union holds the
// day after the first sweep, so the second sweep completes it, torn or not. A rule that
// needed the latest sweep alone to hold the total would leave this day partial.
func TestAPerfectSweepFollowedByATornOneStillCompletesTheDay(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 150)
	sweepServer(h, 150, [][]string{all, tornSweep()})
	h.store.resolve = held(all)
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	if got := h.api.pageNumbers(); !slices.Equal(got, []int{1, 2, 1, 2}) {
		t.Errorf("pages = %v, want [1 2 1 2]", got)
	}
	rec := h.finishedDay(t, d)
	if rec.State != StateDone || rec.Held != 150 {
		t.Errorf("day = %s/%s held %d, want done with all 150 ids held", rec.State, rec.ErrorCode, rec.Held)
	}
}

// Two sweeps that each miss ten different ids, and whose first pages differ, complete
// the day together although neither holds the total alone. The first page of the second
// sweep must count: it is the only place ids 1 to 10 appear.
func TestTwoTornSweepsThatMissDifferentIDsCompleteTheDay(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 150)
	first := slices.Concat(all[10:110], all[100:150]) // never shows ids 1 to 10
	second := slices.Concat(all[:100], all[90:140])   // never shows ids 141 to 150
	sweepServer(h, 150, [][]string{first, second})
	h.store.resolve = held(all)
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	if got := h.api.pageNumbers(); !slices.Equal(got, []int{1, 2, 1, 2}) {
		t.Errorf("pages = %v, want [1 2 1 2]", got)
	}
	rec := h.finishedDay(t, d)
	if rec.State != StateDone || rec.Held != 150 {
		t.Errorf("day = %s/%s held %d, want done with all 150 ids held", rec.State, rec.ErrorCode, rec.Held)
	}
}

// Counts that change only in the second sweep still end the day, on whichever page and
// whichever of the two counts moves. The day stops at the page that disagrees.
func TestACountThatChangesInTheSecondSweepEndsTheDay(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name              string
		page              int // the page of the second sweep that reports other counts
		total, accessible int64
		wantPages         []int
	}{
		{"total on page 1", 1, 149, 150, []int{1, 2, 1}},
		{"accessible on page 1", 1, 150, 151, []int{1, 2, 1}},
		{"total on page 2", 2, 149, 150, []int{1, 2, 1, 2}},
		{"accessible on page 2", 2, 150, 151, []int{1, 2, 1, 2}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			t.Parallel()

			h := newHarness()
			all := ids(1, 150)
			sweep := 0
			h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
				n := pageNum(p)
				if n == 1 {
					sweep++
				}
				if sweep == 2 && n == c.page {
					return searchResp(c.total, c.accessible, pageOf(all, n)), nil
				}
				return searchResp(150, 150, pageOf(all, n)), nil
			}
			d := day(2026, 10, 7)
			h.run(t, h.engine(t, ModeSync), d, d)

			rec := h.finishedDay(t, d)
			if rec.State != StatePartial || rec.ErrorCode != CodeCountChanged {
				t.Errorf("day = %s/%s, want partial/count_changed", rec.State, rec.ErrorCode)
			}
			if got := h.api.pageNumbers(); !slices.Equal(got, c.wantPages) {
				t.Errorf("pages = %v, want %v", got, c.wantPages)
			}
			if len(h.api.downloads) != 0 {
				t.Errorf("downloads = %d, want none", len(h.api.downloads))
			}
		})
	}
}

// An empty page before the last one cannot be right, whatever the counts say.
func TestAnEmptyPageBeforeTheLastIsCountMismatch(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 250)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if pageNum(p) == 2 {
			return searchResp(250, 250, nil), nil
		}
		return searchResp(250, 250, pageOf(all, pageNum(p))), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	if rec.State != StatePartial || rec.ErrorCode != CodeCountMismatch {
		t.Errorf("day = %s/%s, want partial/count_mismatch", rec.State, rec.ErrorCode)
	}
	if got := h.api.pageNumbers(); !slices.Equal(got, []int{1, 2}) {
		t.Errorf("pages = %v, want [1 2]: the day ends at the empty page", got)
	}
}

// Doffin caps numHitsAccessible at 1000 (SOURCES.md). A larger count is not a day the
// sync understands, and it must not send the sync paging without a bound.
func TestAccessibleAboveTheCapIsCountMismatch(t *testing.T) {
	t.Parallel()

	h := newHarness()
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(1001, 1001, ids(1, 100)), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	if rec.State != StatePartial || rec.ErrorCode != CodeCountMismatch {
		t.Errorf("day = %s/%s, want partial/count_mismatch", rec.State, rec.ErrorCode)
	}
	if got := len(h.api.searches); got != 1 {
		t.Errorf("searches = %d, want 1: no page beyond the first", got)
	}
	if len(h.api.downloads) != 0 {
		t.Errorf("downloads = %d, want none", len(h.api.downloads))
	}
}

// A second sweep whose first page already pushes the union past the total ends the day
// at once. Its second page is never asked for.
func TestAUnionAboveTheTotalStopsTheSweepAtOnce(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 150)
	stranger := slices.Concat(all[:95], ids(900001, 5)) // 100 hits, five of them not in the day
	sweep := 0
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		n := pageNum(p)
		if n == 1 {
			sweep++
		}
		if sweep == 2 && n == 1 {
			return searchResp(150, 150, stranger), nil
		}
		return searchResp(150, 150, pageOf(all, n)), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	if rec.State != StatePartial || rec.ErrorCode != CodeCountChanged {
		t.Errorf("day = %s/%s, want partial/count_changed", rec.State, rec.ErrorCode)
	}
	if got := h.api.pageNumbers(); !slices.Equal(got, []int{1, 2, 1}) {
		t.Errorf("pages = %v, want [1 2 1]: the union passed the total on page 1 of sweep 2", got)
	}
}

// A one-page day has no page boundary to reorder across, so every sweep of it must hold
// the whole day, not only the union of the sweeps.
func TestAShortSecondSweepOfAOnePageDayIsCountMismatch(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 10)
	sweep := 0
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		sweep++ // one page per sweep
		if sweep == 2 {
			return searchResp(10, 10, all[:8]), nil
		}
		return searchResp(10, 10, all), nil
	}
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync), d, d)

	rec := h.finishedDay(t, d)
	if rec.State != StatePartial || rec.ErrorCode != CodeCountMismatch {
		t.Errorf("day = %s/%s, want partial/count_mismatch", rec.State, rec.ErrorCode)
	}
	if got := len(h.api.searches); got != 2 {
		t.Errorf("searches = %d, want 2", got)
	}
	if len(h.api.downloads) != 0 {
		t.Errorf("downloads = %d, want none", len(h.api.downloads))
	}
}

// Page size 1 puts every hit on its own page.
func TestPageSizeOfOneSweepsEachHitOnItsOwnPage(t *testing.T) {
	t.Parallel()

	h := newHarness()
	all := ids(1, 3)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(3, 3, pageOfSize(all, pageNum(p), p.NumHitsPerPage)), nil
	}
	h.store.resolve = held(all)
	d := day(2026, 10, 7)
	h.run(t, h.engine(t, ModeSync, func(o *Options) { o.PageSize = 1 }), d, d)

	if got := h.api.pageNumbers(); !slices.Equal(got, []int{1, 2, 3, 1, 2, 3}) {
		t.Errorf("pages = %v, want pages 1 to 3 twice", got)
	}
	if rec := h.finishedDay(t, d); rec.State != StateDone {
		t.Errorf("day = %s/%s, want done", rec.State, rec.ErrorCode)
	}
}

// The ceiling of 100 is the default: both ask for 100 hits a page and label the stored
// pages perPage=100.
func TestPageSizeOfOneHundredIsTheDefault(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		name   string
		mutate func(*Options)
	}{
		{"unset", func(*Options) {}},
		{"explicit 100", func(o *Options) { o.PageSize = 100 }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			h := newHarness()
			all := ids(1, 150)
			h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
				return searchResp(150, 150, pageOfSize(all, pageNum(p), p.NumHitsPerPage)), nil
			}
			h.store.resolve = held(all)
			d := day(2026, 10, 7)
			h.run(t, h.engine(t, ModeSync, tc.mutate), d, d)

			if got := h.api.pageNumbers(); !slices.Equal(got, []int{1, 2, 1, 2}) {
				t.Errorf("pages = %v, want [1 2 1 2]", got)
			}
			for _, c := range h.api.searches {
				if c.Size != 100 {
					t.Errorf("NumHitsPerPage = %d, want 100", c.Size)
				}
			}
			for _, p := range h.store.pages {
				if !strings.Contains(p.QueryKey, "perPage=100;") {
					t.Errorf("query key %q does not carry perPage=100", p.QueryKey)
				}
			}
		})
	}
}

// Each day reports how many searches it took, so a census can show how many sweeps a
// busy day needed. The count is the day's own, not the run's.
func TestDayOutcomeCountsItsOwnSearches(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d1, d2 := day(2026, 10, 6), day(2026, 10, 7)
	all := ids(1, 150)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if p.IssueDateFrom.Equal(d2) {
			return searchResp(150, 150, pageOf(all, pageNum(p))), nil
		}
		return searchResp(0, 0, nil), nil
	}
	h.store.resolve = held(all)
	sum := h.run(t, h.engine(t, ModeSync), d1, d2)

	if len(sum.Days) != 2 {
		t.Fatalf("days = %d, want 2", len(sum.Days))
	}
	if got := sum.Days[0].Searches; got != 2 {
		t.Errorf("zero-hit day searches = %d, want 2: two sweeps of page 1", got)
	}
	if got := sum.Days[1].Searches; got != 4 {
		t.Errorf("150-hit day searches = %d, want 4: two sweeps of two pages", got)
	}
}

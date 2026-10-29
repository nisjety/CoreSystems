package doffinsync

import (
	"errors"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// T34
func TestDoneDaysAreSkippedExceptRecheck(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d1, d2, d3 := day(2026, 10, 4), day(2026, 10, 5), day(2026, 10, 6)
	h.store.dayStates = map[string]DayState{
		"2026-10-04": StateDone,
		"2026-10-05": StateDone,
		"2026-10-06": StateDone,
	}
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(0, 0, nil), nil
	}
	sum := h.run(t, h.engine(t, ModeSync, func(o *Options) { o.Recheck = 2 }), d1, d3)

	got := h.api.searchDays()
	want := []time.Time{d2, d2, d3, d3} // two passes on each of the last two days
	if len(got) != len(want) || !got[0].Equal(want[0]) || !got[1].Equal(want[1]) || !got[2].Equal(want[2]) || !got[3].Equal(want[3]) {
		t.Errorf("searched days = %v, want the last two only, each in two passes (%v)", got, want)
	}
	if sum.Reason != ReasonDone {
		t.Errorf("run reason = %s, want done", sum.Reason)
	}
}

// T35
func TestCensusDownloadsNothing(t *testing.T) {
	t.Parallel()

	h := newHarness()
	cappedDay := day(2026, 10, 3)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if p.IssueDateFrom.Equal(cappedDay) {
			return searchResp(2, 1, ids(1, 1)), nil // capped: T > A
		}
		return searchResp(1, 1, ids(1, 1)), nil
	}
	// Ten closed days, the last one being yesterday.
	sum := h.run(t, h.engine(t, ModeCensus), day(2026, 9, 29), day(2026, 10, 8))

	searched, capped := 0, 0
	for _, rec := range h.store.finished {
		switch rec.State {
		case StateSearched:
			searched++
		case StateCapped:
			capped++
		}
	}
	switch {
	case len(h.api.downloads) != 0:
		t.Errorf("downloads = %d, want none in census", len(h.api.downloads))
	case searched != 9 || capped != 1:
		t.Errorf("states: %d searched and %d capped, want 9 and 1", searched, capped)
	case sum.Reason != ReasonDone:
		t.Errorf("run reason = %s, want done: searched and capped days are not gaps in census", sum.Reason)
	}
}

// T36
func TestMaxDownloadsStopsAtLimit(t *testing.T) {
	t.Parallel()

	h := newHarness()
	d1, d2 := day(2026, 10, 6), day(2026, 10, 7)
	all := ids(1, 2)
	h.api.search = func(_ int, p doffin.SearchParams) (doffin.SearchResponse, error) {
		if p.IssueDateFrom.Equal(d1) {
			return searchResp(2, 2, all), nil
		}
		return searchResp(0, 0, nil), nil
	}
	sum := h.run(t, h.engine(t, ModeSync, func(o *Options) { o.MaxDownloads = 1 }), d1, d2)

	switch {
	case len(h.api.downloads) != 1:
		t.Errorf("downloads = %d, want 1", len(h.api.downloads))
	case len(h.api.searchDays()) != 2:
		t.Errorf("searches = %d, want 2: two passes on the first day, the next day untouched", len(h.api.searchDays()))
	case sum.Reason != ReasonPartial:
		t.Errorf("run reason = %s, want partial", sum.Reason)
	}
	if rec := h.finishedDay(t, d1); rec.State != StatePartial || rec.ErrorCode != CodeDownloadLimit {
		t.Errorf("day = %s/%s, want partial/download_limit", rec.State, rec.ErrorCode)
	}
}

// T37
func TestLedgerErrorFailsRunWithoutDownloads(t *testing.T) {
	t.Parallel()

	h := newHarness()
	sentinel := errors.New("ledger unavailable")
	h.store.beginDayErr = sentinel
	d := day(2026, 10, 7)
	all := ids(1, 2)
	h.api.search = func(int, doffin.SearchParams) (doffin.SearchResponse, error) {
		return searchResp(2, 2, all), nil
	}
	_, err := h.engine(t, ModeSync).Run(t.Context(), Window{From: d, To: d})

	if !errors.Is(err, sentinel) {
		t.Errorf("Run error = %v, want the ledger error", err)
	}
	if len(h.api.downloads) != 0 {
		t.Errorf("downloads = %d, want none after a ledger failure", len(h.api.downloads))
	}
	if len(h.store.runOutcomes) != 1 || h.store.runOutcomes[0].Reason != ReasonFailed {
		t.Errorf("run outcomes = %+v, want one failed outcome", h.store.runOutcomes)
	}
}

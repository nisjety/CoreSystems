package incumbency

import (
	"slices"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
)

// The radar lists contracts whose end falls in the window, nearest first, for the profile's CPV and
// region. A contract with no end that can be told (open, or no period) is not on it: it cannot be
// said to be ending.
func TestReTendersListsContractsEndingInTheWindow(t *testing.T) {
	t.Parallel()

	twoYears := &eforms.ContractPeriod{Duration: &eforms.Duration{Value: 24, Unit: "MONTH"}}
	awards := []Award{
		one(t, result{notice: "soon", procedure: "p1", buyer: buyer, cpv: "72212000", awarded: day(2025, 1, 1), period: twoYears, winners: []eforms.Winner{acme}}),
		one(t, result{notice: "later", procedure: "p2", buyer: buyer, cpv: "72212000", awarded: day(2025, 6, 1), period: twoYears, winners: []eforms.Winner{beta}}),
		one(t, result{notice: "too late", procedure: "p3", buyer: buyer, cpv: "72212000", awarded: day(2026, 1, 1), period: twoYears, winners: []eforms.Winner{acme}}),
		one(t, result{notice: "past", procedure: "p4", buyer: buyer, cpv: "72212000", awarded: day(2023, 1, 1), period: twoYears, winners: []eforms.Winner{acme}}),
		one(t, result{notice: "other cpv", procedure: "p5", buyer: buyer, cpv: "45000000", awarded: day(2025, 1, 1), period: twoYears, winners: []eforms.Winner{acme}}),
		one(t, result{notice: "open", procedure: "p6", buyer: buyer, cpv: "72212000", awarded: day(2025, 1, 1), period: &eforms.ContractPeriod{Unlimited: true}, winners: []eforms.Winner{acme}}),
		one(t, result{notice: "no period", procedure: "p7", buyer: buyer, cpv: "72212000", awarded: day(2025, 1, 1), winners: []eforms.Winner{acme}}),
	}
	w := Window{From: time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC), To: time.Date(2027, 9, 30, 0, 0, 0, 0, time.UTC)}
	got := ReTenders(awards, Scope{CPV: []string{"72000000"}}, w, classes)
	var ids []string
	for _, e := range got {
		ids = append(ids, e.NoticeID)
	}
	if want := []string{"soon", "later"}; !slices.Equal(ids, want) {
		t.Errorf("radar = %v, want %v", ids, want)
	}
	if got[0].End.Basis != EndFromAward || got[0].End.At == nil || !got[0].End.At.Equal(time.Date(2027, 1, 1, 0, 0, 0, 0, time.UTC)) {
		t.Errorf("end of the first = %+v", got[0].End)
	}
	if !slices.Equal(got[0].BuyerNames, []string{"Eksempelkommune"}) {
		t.Errorf("the buyer is not shown: %v", got[0].BuyerNames)
	}
}

// A region in the scope narrows the radar to contracts in it, or covering it; a contract that
// states no region is not dropped for it, as the gate treats an unknown.
func TestReTendersByRegion(t *testing.T) {
	t.Parallel()

	twoYears := &eforms.ContractPeriod{Duration: &eforms.Duration{Value: 24, Unit: "MONTH"}}
	oslo := one(t, result{notice: "oslo", procedure: "p1", buyer: buyer, cpv: "72212000", awarded: day(2025, 1, 1), period: twoYears, winners: []eforms.Winner{acme}})
	bergen := one(t, result{notice: "bergen", procedure: "p2", buyer: buyer, cpv: "72212000", awarded: day(2025, 1, 1), period: twoYears, winners: []eforms.Winner{acme}})
	bergen.NUTS = []string{"NO0A1"}
	nowhere := one(t, result{notice: "nowhere", procedure: "p3", buyer: buyer, cpv: "72212000", awarded: day(2025, 1, 1), period: twoYears, winners: []eforms.Winner{acme}})
	nowhere.NUTS = nil

	w := Window{From: time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC), To: time.Date(2028, 1, 1, 0, 0, 0, 0, time.UTC)}
	got := ReTenders([]Award{oslo, bergen, nowhere}, Scope{NUTS: []string{"NO08"}}, w, classes)
	var ids []string
	for _, e := range got {
		ids = append(ids, e.NoticeID)
	}
	slices.Sort(ids)
	if want := []string{"nowhere", "oslo"}; !slices.Equal(ids, want) {
		t.Errorf("radar for NO08 = %v, want %v", ids, want)
	}
}

// The radar is capped, and the winners on it follow the panel's rule.
func TestReTendersCapsAndWithholds(t *testing.T) {
	t.Parallel()

	twoYears := &eforms.ContractPeriod{Duration: &eforms.Duration{Value: 24, Unit: "MONTH"}}
	var awards []Award
	for i := 0; i < MaxReTenders+3; i++ {
		awards = append(awards, one(t, result{notice: "n" + string(rune('a'+i)), procedure: "p" + string(rune('a'+i)), buyer: buyer, cpv: "72212000",
			awarded: day(2025, 1, 1+i), period: twoYears, winners: []eforms.Winner{solo}}))
	}
	w := Window{From: time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC), To: time.Date(2028, 1, 1, 0, 0, 0, 0, time.UTC)}
	got := ReTenders(awards, Scope{}, w, classes)
	if len(got) != MaxReTenders {
		t.Errorf("radar has %d entries, want the cap %d", len(got), MaxReTenders)
	}
	if got[0].Winners[0] != (WinnerView{Class: SoleTrader}) {
		t.Errorf("a sole trader is named on the radar: %+v", got[0].Winners)
	}
}

// Contracts that end on the same day come out in notice-id order, whatever order the store read them
// in, so the radar does not reorder between two reads of the same notices.
func TestReTendersOrdersTiesByNoticeIDNotByReadOrder(t *testing.T) {
	t.Parallel()

	twoYears := &eforms.ContractPeriod{Duration: &eforms.Duration{Value: 24, Unit: "MONTH"}}
	sameDay := func(notice string) Award {
		return one(t, result{notice: notice, procedure: "p-" + notice, buyer: buyer, cpv: "72212000", awarded: day(2025, 1, 1), period: twoYears, winners: []eforms.Winner{acme}})
	}
	w := Window{From: time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC), To: time.Date(2028, 1, 1, 0, 0, 0, 0, time.UTC)}
	for _, read := range [][]string{{"c", "a", "b"}, {"b", "c", "a"}} {
		var awards []Award
		for _, n := range read {
			awards = append(awards, sameDay(n))
		}
		var ids []string
		for _, e := range ReTenders(awards, Scope{}, w, classes) {
			ids = append(ids, e.NoticeID)
		}
		if want := []string{"a", "b", "c"}; !slices.Equal(ids, want) {
			t.Errorf("read as %v the radar lists %v, want %v", read, ids, want)
		}
	}
}

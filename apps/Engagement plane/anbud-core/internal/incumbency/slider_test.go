package incumbency

import (
	"fmt"
	"slices"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/match"
)

// sliderSpan is the window of the slider tests: the two years from the start of 2026.
func sliderSpan() Window {
	return Window{From: time.Date(2026, time.January, 1, 0, 0, 0, 0, time.UTC), To: time.Date(2028, time.January, 1, 0, 0, 0, 0, time.UTC)}
}

// awardedOn is an award of one buyer for two years from the given day.
func awardedOn(t *testing.T, notice, cpv string, awarded time.Time) Award {
	t.Helper()
	return one(t, result{
		notice: notice, procedure: "p-" + notice, buyer: buyer, cpv: cpv, awarded: &awarded,
		period:  &eforms.ContractPeriod{Duration: &eforms.Duration{Value: 24, Unit: "MONTH"}},
		winners: []eforms.Winner{acme},
	})
}

// endingIn2027 is an award from January 2025: its contract ends inside sliderSpan.
func endingIn2027(t *testing.T, notice, cpv string) Award {
	return awardedOn(t, notice, cpv, time.Date(2025, time.January, 1, 0, 0, 0, 0, time.UTC))
}

func noticesOf(as []Award) []string {
	out := make([]string, 0, len(as))
	for _, a := range as {
		out = append(out, a.NoticeID)
	}
	slices.Sort(out)
	return out
}

func TestSliderWidensTheCPVScopeOneStepAtATime(t *testing.T) {
	t.Parallel()

	awards := []Award{
		endingIn2027(t, "a", "72212000"), // the profile's own code
		endingIn2027(t, "b", "72000000"), // a division covering it
		endingIn2027(t, "c", "72215000"), // the same group
		endingIn2027(t, "d", "72300000"), // the same division only
		endingIn2027(t, "e", "45000000"), // another division
	}
	want := map[match.Strictness][]string{
		match.Strict:   {"a", "b"},
		match.Balanced: {"a", "b", "c"},
		match.Wide:     {"a", "b", "c", "d"},
	}
	for level, ids := range want {
		got := Ending(awards, Scope{CPV: []string{"72212000"}, Level: level}, sliderSpan())
		if !slices.Equal(noticesOf(got), ids) {
			t.Errorf("CPV scope at %v lists %v, want %v", level, noticesOf(got), ids)
		}
	}
}

func TestSliderWidensTheRegionScopeOneStepAtATime(t *testing.T) {
	t.Parallel()

	var awards []Award
	for id, region := range map[string]string{"a": "NO081", "b": "NO082", "c": "NO0A1", "d": "SE111"} {
		aw := endingIn2027(t, id, "72212000")
		aw.NUTS = []string{region}
		awards = append(awards, aw)
	}
	want := map[match.Strictness][]string{
		match.Strict:   {"a"},
		match.Balanced: {"a", "b"},
		match.Wide:     {"a", "b", "c"},
	}
	for level, ids := range want {
		got := Ending(awards, Scope{NUTS: []string{"NO081"}, Level: level}, sliderSpan())
		if !slices.Equal(noticesOf(got), ids) {
			t.Errorf("region scope at %v lists %v, want %v", level, noticesOf(got), ids)
		}
	}
}

// A scope that gives no level is strict, as before the slider existed.
func TestAScopeWithNoLevelIsStrict(t *testing.T) {
	t.Parallel()

	awards := []Award{endingIn2027(t, "a", "72212000"), endingIn2027(t, "c", "72215000")}
	got := Ending(awards, Scope{CPV: []string{"72212000"}}, sliderSpan())
	if !slices.Equal(noticesOf(got), []string{"a"}) {
		t.Errorf("a scope with no level lists %v, want only the exact code", noticesOf(got))
	}
}

// An award that states no code of a kind is never narrowed away by that kind, at any step.
func TestAnAwardWithNoCodeIsNotNarrowedAway(t *testing.T) {
	t.Parallel()

	aw := endingIn2027(t, "x", "72212000")
	aw.CPV, aw.NUTS = nil, nil
	for _, level := range []match.Strictness{match.Strict, match.Balanced, match.Wide} {
		got := Ending([]Award{aw}, Scope{CPV: []string{"45000000"}, NUTS: []string{"SE111"}, Level: level}, sliderSpan())
		if len(got) != 1 {
			t.Errorf("at %v the award with no codes is listed %d times, want once", level, len(got))
		}
	}
}

// The slider's "+N if you widen": for each wider step, the contracts it adds over the current step,
// counting only contracts that end in the window.
func TestWideningCountsWhatEachWiderStepAddsWithinTheWindow(t *testing.T) {
	t.Parallel()

	awards := []Award{
		endingIn2027(t, "a", "72212000"),
		endingIn2027(t, "b", "72000000"),
		endingIn2027(t, "c", "72215000"),
		endingIn2027(t, "d", "72300000"),
		endingIn2027(t, "e", "45000000"),
		awardedOn(t, "old", "72300000", time.Date(2019, time.January, 1, 0, 0, 0, 0, time.UTC)), // ended in 2021
	}
	got := Widening(awards, Scope{CPV: []string{"72212000"}, Level: match.Strict}, sliderSpan())
	want := []Widen{{Level: match.Balanced, Adds: 1}, {Level: match.Wide, Adds: 2}}
	if !slices.Equal(got, want) {
		t.Errorf("widening from strict = %+v, want %+v", got, want)
	}
	got = Widening(awards, Scope{CPV: []string{"72212000"}, Level: match.Balanced}, sliderSpan())
	if !slices.Equal(got, []Widen{{Level: match.Wide, Adds: 1}}) {
		t.Errorf("widening from balanced = %+v, want the wide step adding one", got)
	}
	if got := Widening(awards, Scope{CPV: []string{"72212000"}, Level: match.Wide}, sliderSpan()); len(got) != 0 {
		t.Errorf("widening from wide = %+v, want nothing: wide is the widest", got)
	}
}

// capCorpus is forty contracts of the profile's own code and forty of its group. The group's contracts end
// first, so an end-date order alone would cut the exact matches off the radar's list.
func capCorpus(t *testing.T) []Award {
	t.Helper()
	twoYears := &eforms.ContractPeriod{Duration: &eforms.Duration{Value: 24, Unit: "MONTH"}}
	var awards []Award
	for i := 0; i < 40; i++ {
		awards = append(awards,
			one(t, result{notice: fmt.Sprintf("own-%02d", i), procedure: fmt.Sprintf("po-%02d", i), buyer: buyer, cpv: "72212000", awarded: day(2025, 6, 1+i), period: twoYears, winners: []eforms.Winner{acme}}),
			one(t, result{notice: fmt.Sprintf("grp-%02d", i), procedure: fmt.Sprintf("pg-%02d", i), buyer: buyer, cpv: "72215000", awarded: day(2025, 1, 1+i), period: twoYears, winners: []eforms.Winner{acme}}))
	}
	return awards
}

// inOrder lists the notice ids of the awards as they come, where noticesOf sorts them.
func inOrder(as []Award) []string {
	out := make([]string, 0, len(as))
	for _, a := range as {
		out = append(out, a.NoticeID)
	}
	return out
}

// At a wider step the cut keeps the strictest step's matches: the list at strict is the start of the list at
// balanced, and the total says how many matched before the cut.
func TestReTendersKeepsExactMatchesWhenTheCapBinds(t *testing.T) {
	t.Parallel()

	awards := capCorpus(t)
	w := sliderSpan()
	strict := Scope{CPV: []string{"72212000"}, Level: match.Strict}
	balanced := Scope{CPV: []string{"72212000"}, Level: match.Balanced}

	exact := inOrder(Ending(awards, strict, w))
	listed := inOrder(Ending(awards, balanced, w))
	if len(listed) != MaxReTenders {
		t.Fatalf("balanced lists %d contracts, want the cap %d", len(listed), MaxReTenders)
	}
	if !slices.Equal(listed[:len(exact)], exact) {
		t.Errorf("the strict list %v does not start the balanced list %v", exact, listed)
	}
	if got := Matching(awards, strict, w); got != 40 {
		t.Errorf("strict matches %d, want 40", got)
	}
	if got := Matching(awards, balanced, w); got != 80 {
		t.Errorf("balanced matches %d, want 80", got)
	}
}

// The "+N" counts every contract a wider step matches, past the cap, so it agrees with the total.
func TestWideningCountsPastTheCap(t *testing.T) {
	t.Parallel()

	got := Widening(capCorpus(t), Scope{CPV: []string{"72212000"}, Level: match.Strict}, sliderSpan())
	want := []Widen{{Level: match.Balanced, Adds: 40}, {Level: match.Wide, Adds: 40}}
	if !slices.Equal(got, want) {
		t.Errorf("widening = %+v, want %+v", got, want)
	}
}

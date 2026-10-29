package demo

import (
	"encoding/json"
	"maps"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/match"
)

func TestMoneyIsExactToTheOre(t *testing.T) {
	t.Parallel()

	for minor, want := range map[int64]string{0: "0.00", 5: "0.05", 100: "1.00", 150_000_050: "1500000.50", 99_999_999_999_999_999: "999999999999999.99"} {
		got := money(&eforms.Amount{Minor: minor, Currency: "NOK"})
		if got == nil || got.Currency != "NOK" || got.Amount != want {
			t.Errorf("money(%d) = %+v, want %s", minor, got, want)
		}
	}
	if money(nil) != nil {
		t.Errorf("money(nil) is not nil")
	}
}

func TestClipKeepsWhatFitsAndMarksWhatItCut(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct{ in, want string }{
		{"abc", "abc"}, {"abcd", "abc…"}, {"æøå", "æøå"}, {"æøåx", "æøå…"}, {"", ""},
	} {
		if got := clip(tc.in, 3); got != tc.want {
			t.Errorf("clip(%q, 3) = %q, want %q", tc.in, got, tc.want)
		}
	}
}

func TestCappedListsNoMoreThanTheCapAndCountsAll(t *testing.T) {
	t.Parallel()

	for _, n := range []int{0, 1, maxListed - 1, maxListed, maxListed + 1, maxListed * 3} {
		items := make([]int, n)
		got, total := capped(items)
		if want := min(n, maxListed); len(got) != want || total != n {
			t.Errorf("capped of %d: listed %d total %d, want %d of %d", n, len(got), total, want, n)
		}
	}
}

func TestACheckNamesItsIndexOnlyWhereItMeansSomething(t *testing.T) {
	t.Parallel()

	views := checkViews([]match.Check{
		{Rule: match.RuleDeadline, Outcome: match.Pass, Cite: match.Cite{Field: "deadline", Lot: "LOT-0001"}},
		{Rule: match.RuleRequirement, Outcome: match.Unknown, Cite: match.Cite{Field: "selection criterion", Index: 0}},
		{Rule: match.RuleDocuments, Outcome: match.Unknown, Cite: match.Cite{Field: "requirement", Index: 2}},
	})
	if views[0].Cite.Index != nil {
		t.Errorf("a deadline has an index of %d", *views[0].Cite.Index)
	}
	if views[1].Cite.Index == nil || *views[1].Cite.Index != 0 || views[2].Cite.Index == nil || *views[2].Cite.Index != 2 {
		t.Errorf("indexes = %v %v: the first criterion is index 0, and that must be said", views[1].Cite.Index, views[2].Cite.Index)
	}
	out, _ := json.Marshal(views[0])
	if strings.Contains(string(out), "index") {
		t.Errorf("a deadline check says %s", out)
	}
}

// A list that is empty is [] in JSON, not null: a client iterates it without a check.
func TestAResultHasNoNullLists(t *testing.T) {
	t.Parallel()

	out, err := json.Marshal(newResult("2026-100001", candidate{}, match.Assessment{}, match.Evidence{}, now))
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	for _, key := range []string{"cpv", "nuts", "buyer_org_numbers", "gate", "fit"} {
		if !strings.Contains(string(out), `"`+key+`":[]`) {
			t.Errorf("%s is not an empty list in %s", key, out)
		}
	}
}

func TestAnAuditHasNoNullLists(t *testing.T) {
	t.Parallel()

	out, err := json.Marshal(newAudit(doc("2026-100001", []byte("x")), candidate{}, match.Assessment{}, match.Evidence{}, now))
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	for _, key := range []string{"lots", "selection_criteria", "documents", "changes", "problems", "issues", "unread"} {
		if !strings.Contains(string(out), `"`+key+`":[]`) {
			t.Errorf("%s is not an empty list in %s", key, out)
		}
	}
}

// GateCounts lets a client tell a notice that is conditional because a requirement is not met in
// time from one that is conditional only because nobody has read the documents yet.
func TestAResultCountsItsGateChecksByOutcome(t *testing.T) {
	t.Parallel()

	a := match.Assessment{Gate: []match.Check{
		{Outcome: match.Pass}, {Outcome: match.Unknown}, {Outcome: match.Unknown}, {Outcome: match.Conditional}, {Outcome: match.Fail},
	}}
	got := newResult("2026-100001", candidate{}, a, match.Evidence{}, now).GateCounts
	if want := map[string]int{"pass": 1, "conditional": 1, "unknown": 2, "fail": 1}; !maps.Equal(got, want) {
		t.Errorf("gate counts = %v, want %v", got, want)
	}
	if empty := newResult("2026-100001", candidate{}, match.Assessment{}, match.Evidence{}, now).GateCounts; !maps.Equal(empty, map[string]int{"pass": 0, "conditional": 0, "unknown": 0, "fail": 0}) {
		t.Errorf("with no checks: %v, want all four outcomes present at zero", empty)
	}
}

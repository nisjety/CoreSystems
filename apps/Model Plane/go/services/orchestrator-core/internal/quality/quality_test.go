package quality

import (
	"math"
	"testing"
)

func approx(t *testing.T, got, want, tolerance float64, label string) {
	t.Helper()
	if math.Abs(got-want) > tolerance {
		t.Fatalf("%s = %.4f, want ~%.4f", label, got, want)
	}
}

// ── The reason a Wilson bound replaced a raw ratio ──────────────────────────

// The headline case. A raw ratio calls 1-of-1 a perfect skill; the bound calls it
// what it is — almost no evidence.
func TestOnePerfectRatingIsNotAPromotableSkill(t *testing.T) {
	score := Evaluate(Evidence{ExplicitGood: 1})
	if score.LowerBound >= PromoteBound {
		t.Fatalf("1-of-1 scored %.3f, which would promote on a single rating", score.LowerBound)
	}
	// And the raw ratio it replaced would have.
	if raw := 1.0 / 1.0; raw < PromoteBound {
		t.Fatal("the raw ratio should have promoted; this test documents the difference")
	}
}

// The mirror case: a single bad rating must not quarantine anything, both because
// the bound stays high and because the evidence floor is not met.
func TestOneBadRatingCannotQuarantine(t *testing.T) {
	score := Evaluate(Evidence{ExplicitBad: 1})
	if got := Decide(StateActive, score); got != DecisionHold {
		t.Fatalf("decision = %q on one bad rating, want hold", got)
	}
}

// Volume must matter, not just agreement — the property a raw ratio lacks.
func TestTheBoundRisesWithVolumeAtTheSameRatio(t *testing.T) {
	small := Evaluate(Evidence{ExplicitGood: 5})
	large := Evaluate(Evidence{ExplicitGood: 50})
	if !(large.LowerBound > small.LowerBound) {
		t.Fatalf("50-of-50 (%.3f) should outrank 5-of-5 (%.3f)", large.LowerBound, small.LowerBound)
	}
	if small.LowerBound >= PromoteBound {
		t.Fatalf("5-of-5 scored %.3f; too thin to promote", small.LowerBound)
	}
	if large.LowerBound < PromoteBound {
		t.Fatalf("50-of-50 scored %.3f; should be promotable", large.LowerBound)
	}
}

func TestWilsonLowerBoundKnownValues(t *testing.T) {
	// Reference values for the 95% Wilson lower bound.
	approx(t, WilsonLowerBound(1, 1, Wilson95Z), 0.2065, 0.01, "1/1")
	approx(t, WilsonLowerBound(10, 10, Wilson95Z), 0.7225, 0.01, "10/10")
	approx(t, WilsonLowerBound(5, 10, Wilson95Z), 0.2366, 0.01, "5/10")
	approx(t, WilsonLowerBound(0, 10, Wilson95Z), 0.0, 0.01, "0/10")
}

func TestWilsonLowerBoundIsBoundedAndSafeOnEdges(t *testing.T) {
	for _, tc := range []struct{ good, total float64 }{
		{0, 0}, {1, 0}, {-1, 5}, {5, 5}, {6, 5}, {0.5, 1.5},
	} {
		got := WilsonLowerBound(tc.good, tc.total, Wilson95Z)
		if got < 0 || got > 1 || math.IsNaN(got) {
			t.Fatalf("WilsonLowerBound(%v, %v) = %v, must stay in [0,1]", tc.good, tc.total, got)
		}
	}
	// No observations must neither promote nor demote.
	if got := WilsonLowerBound(0, 0, Wilson95Z); got != 0 {
		t.Fatalf("no evidence scored %v, want 0", got)
	}
}

// ── Implicit weighting ──────────────────────────────────────────────────────

// The signal that must not be able to demote on its own. Someone clicking
// regenerate for variety is the most common false positive available.
func TestRepeatedRegeneratesAloneCannotQuarantine(t *testing.T) {
	// 20 regenerates: each 0.3 detector strength × 0.25 implicit weight = 0.075.
	score := Evaluate(Evidence{ImplicitBadWeight: 20 * 0.3})
	if score.WeightedTotal >= MinEvidenceForDemotion {
		t.Fatalf("20 regenerates reached %.2f weighted samples; the floor is meant to hold them off",
			score.WeightedTotal)
	}
	if got := Decide(StateActive, score); got != DecisionHold {
		t.Fatalf("decision = %q, want hold: behaviour alone must not demote", got)
	}
}

// A handful of deliberate thumbs-down, by contrast, should act.
func TestExplicitNegativesQuarantinePromptly(t *testing.T) {
	score := Evaluate(Evidence{ExplicitBad: 4})
	if got := Decide(StateActive, score); got != DecisionQuarantine {
		t.Fatalf("decision = %q on 4 explicit negatives, want quarantine (bound %.3f, total %.1f)",
			got, score.LowerBound, score.WeightedTotal)
	}
}

func TestOneExplicitNegativeOutweighsSeveralImplicitOnes(t *testing.T) {
	explicit := Evaluate(Evidence{ExplicitBad: 1}).WeightedTotal
	implicit := Evaluate(Evidence{ImplicitBadWeight: 3 * 1.0}).WeightedTotal // 3 corrections
	if explicit <= implicit {
		t.Fatalf("one explicit negative (%.2f) should outweigh three corrections (%.2f)",
			explicit, implicit)
	}
}

// Silence is not approval. There is no behavioural signal for satisfaction, so
// accepting one would make every ignored turn a positive vote.
func TestThereIsNoImplicitPositiveByDefault(t *testing.T) {
	score := Evaluate(Evidence{ImplicitBadWeight: 1})
	if score.WeightedGood != 0 {
		t.Fatalf("WeightedGood = %v with no explicit positives, want 0", score.WeightedGood)
	}
}

func TestHasExplicitTracksHumanJudgementOnly(t *testing.T) {
	if Evaluate(Evidence{ImplicitBadWeight: 10}).HasExplicit {
		t.Fatal("implicit signals must not be reported as human judgement")
	}
	if !Evaluate(Evidence{ExplicitBad: 1}).HasExplicit {
		t.Fatal("an explicit rating must be reported as human judgement")
	}
}

// ── Lifecycle ───────────────────────────────────────────────────────────────

// The single most important property: this package can never retire anything.
func TestQuarantineIsNeverTerminal(t *testing.T) {
	bad := Evaluate(Evidence{ExplicitBad: 20})
	for _, state := range []State{StateActive, StateQuarantined, StateRetired} {
		if got := Decide(state, bad); got == "retire" || got == Decision(StateRetired) {
			t.Fatalf("state %q produced %q; retirement must stay a human decision", state, got)
		}
	}
}

func TestRetiredSkillsAreLeftAlone(t *testing.T) {
	for _, score := range []Score{
		Evaluate(Evidence{ExplicitGood: 100}),
		Evaluate(Evidence{ExplicitBad: 100}),
	} {
		if got := Decide(StateRetired, score); got != DecisionHold {
			t.Fatalf("decision = %q on a retired skill, want hold", got)
		}
	}
}

// Hysteresis: the gap between demote and recover is what stops a skill sitting
// near the line from flipping on alternating sweeps and changing what users see.
func TestHysteresisPreventsFlapping(t *testing.T) {
	if !(RecoverBound > DemoteBound) {
		t.Fatal("RecoverBound must exceed DemoteBound or the loop will flap")
	}
	// A bound inside the band holds in BOTH states — that is the whole point.
	inBand := Score{LowerBound: (DemoteBound + RecoverBound) / 2, WeightedTotal: 100}
	if got := Decide(StateQuarantined, inBand); got != DecisionHold {
		t.Fatalf("quarantined + in-band = %q, want hold", got)
	}
	if got := Decide(StateActive, inBand); got != DecisionHold {
		t.Fatalf("active + in-band = %q, want hold", got)
	}
}

func TestAQuarantinedSkillRecoversOnlyAboveTheRecoverBound(t *testing.T) {
	just_below := Score{LowerBound: RecoverBound - 0.01, WeightedTotal: 100}
	if got := Decide(StateQuarantined, just_below); got != DecisionHold {
		t.Fatalf("decision = %q just below the recover bound, want hold", got)
	}
	at := Score{LowerBound: RecoverBound, WeightedTotal: 100}
	if got := Decide(StateQuarantined, at); got != DecisionRecover {
		t.Fatalf("decision = %q at the recover bound, want recover", got)
	}
}

// A quarantined skill must not jump straight to promotion: the evidence that
// redeemed it is by definition thin.
func TestAQuarantinedSkillCannotBePromotedDirectly(t *testing.T) {
	excellent := Score{LowerBound: 0.99, WeightedTotal: 500}
	if got := Decide(StateQuarantined, excellent); got != DecisionRecover {
		t.Fatalf("decision = %q, want recover rather than promote", got)
	}
}

// An unknown future state must not be treated as active.
func TestAnUnknownStateFailsClosed(t *testing.T) {
	if got := Decide(State("shadow"), Evaluate(Evidence{ExplicitBad: 50})); got != DecisionHold {
		t.Fatalf("decision = %q on an unknown state, want hold", got)
	}
}

// ── Blast radius ────────────────────────────────────────────────────────────

// The worst realistic case is a bad deploy making every answer poor at once. The
// right response is a handful of quarantines and a loud count, not an emptied
// catalogue.
func TestASweepCannotQuarantineEverything(t *testing.T) {
	candidates := make([]Candidate, 0, 20)
	for i := range 20 {
		candidates = append(candidates, Candidate{
			OrgID:   "org",
			SkillID: string(rune('a' + i)),
			State:   StateActive,
			Score:   Score{LowerBound: 0.01 * float64(i), WeightedTotal: 50},
			Action:  DecisionQuarantine,
		})
	}
	actionable, withheld := PlanSweep(candidates)
	if len(actionable) != MaxQuarantinesPerSweep {
		t.Fatalf("acted on %d, want the cap of %d", len(actionable), MaxQuarantinesPerSweep)
	}
	if withheld != 20-MaxQuarantinesPerSweep {
		t.Fatalf("withheld %d, want %d — a silent cap reads as 'nothing more was wrong'",
			withheld, 20-MaxQuarantinesPerSweep)
	}
}

// If the cap bites, the worst skills must be the ones actually stopped.
func TestTheCapKeepsTheWorstOffenders(t *testing.T) {
	candidates := []Candidate{
		{OrgID: "o", SkillID: "mild", State: StateActive, Action: DecisionQuarantine,
			Score: Score{LowerBound: 0.44, WeightedTotal: 50}},
		{OrgID: "o", SkillID: "awful", State: StateActive, Action: DecisionQuarantine,
			Score: Score{LowerBound: 0.01, WeightedTotal: 50}},
	}
	actionable, _ := PlanSweep(candidates)
	if actionable[0].SkillID != "awful" {
		t.Fatalf("first action is %q, want the worst offender first", actionable[0].SkillID)
	}
}

// Promotions and recoveries are not capped: they neither remove capability nor
// change what a user is served in a way that needs bounding.
func TestPromotionsAndRecoveriesAreNotCapped(t *testing.T) {
	candidates := make([]Candidate, 0, 20)
	for i := range 20 {
		candidates = append(candidates, Candidate{
			OrgID: "o", SkillID: string(rune('a' + i)), State: StateActive,
			Action: DecisionPromote, Score: Score{LowerBound: 0.9, WeightedTotal: 100},
		})
	}
	actionable, withheld := PlanSweep(candidates)
	if len(actionable) != 20 || withheld != 0 {
		t.Fatalf("acted on %d (withheld %d); promotions must not be capped", len(actionable), withheld)
	}
}

// Holds are dropped rather than carried through as no-ops.
func TestHoldsAreNotCarriedThroughTheSweep(t *testing.T) {
	actionable, withheld := PlanSweep([]Candidate{
		{OrgID: "o", SkillID: "a", Action: DecisionHold},
		{OrgID: "o", SkillID: "b", Action: DecisionHold},
	})
	if len(actionable) != 0 || withheld != 0 {
		t.Fatalf("actionable = %v, withheld = %d, want both empty", actionable, withheld)
	}
}

// Two sweeps over the same data must act on the same skills, or an operator
// cannot reason about what a sweep did.
func TestSweepPlanningIsDeterministic(t *testing.T) {
	build := func() []Candidate {
		return []Candidate{
			{OrgID: "o", SkillID: "b", Action: DecisionQuarantine, Score: Score{LowerBound: 0.2}},
			{OrgID: "o", SkillID: "a", Action: DecisionQuarantine, Score: Score{LowerBound: 0.2}},
			{OrgID: "o", SkillID: "c", Action: DecisionQuarantine, Score: Score{LowerBound: 0.1}},
		}
	}
	first, _ := PlanSweep(build())
	second, _ := PlanSweep(build())
	for i := range first {
		if first[i].SkillID != second[i].SkillID {
			t.Fatalf("sweep order differs at %d: %q vs %q", i, first[i].SkillID, second[i].SkillID)
		}
	}
	// Ties broken by id, so "a" precedes "b" at equal bounds.
	if first[0].SkillID != "c" || first[1].SkillID != "a" || first[2].SkillID != "b" {
		t.Fatalf("order = %q/%q/%q, want c/a/b (worst first, ties by id)",
			first[0].SkillID, first[1].SkillID, first[2].SkillID)
	}
}

// ── Bound sanity ────────────────────────────────────────────────────────────

func TestThePolicyBoundsAreOrdered(t *testing.T) {
	if !(DemoteBound < RecoverBound && RecoverBound <= PromoteBound) {
		t.Fatalf("bounds out of order: demote %.2f, recover %.2f, promote %.2f",
			DemoteBound, RecoverBound, PromoteBound)
	}
	if ImplicitWeight <= 0 || ImplicitWeight >= 1 {
		t.Fatalf("ImplicitWeight = %v, must discount implicit evidence without erasing it",
			ImplicitWeight)
	}
}

package scoring

import (
	"math"
	"testing"
)

func approx(a, b float64) bool { return math.Abs(a-b) < 1e-9 }

func TestScore_PerfectCapabilityIsOne(t *testing.T) {
	m := Metrics{
		SuccessRate:    1.0,
		SchemaFailRate: 0.0,
		P95LatencyMS:   latencyFloorMS, // ideal latency
		MeanCostUSD:    0.0,
		ApprovalRate:   1.0,
		IncidentCount:  0,
		OperatorRating: operatorMax,
		RolloutState:   "stable",
	}
	got := Score(m, DefaultWeights())
	if !approx(got, 1.0) {
		t.Fatalf("perfect capability should score 1.0, got %v", got)
	}
}

func TestScore_WorstCapabilityIsZero(t *testing.T) {
	m := Metrics{
		SuccessRate:    0.0,
		SchemaFailRate: 1.0,
		P95LatencyMS:   latencyCeilMS,
		MeanCostUSD:    costCeilUSD,
		ApprovalRate:   0.0,
		IncidentCount:  int(incidentCeil),
		OperatorRating: 0.0,
		RolloutState:   "stable",
	}
	got := Score(m, DefaultWeights())
	if !approx(got, 0.0) {
		t.Fatalf("worst capability should score 0.0, got %v", got)
	}
}

func TestScore_RolloutStateRanksBelowStable(t *testing.T) {
	base := Metrics{
		SuccessRate: 0.9, SchemaFailRate: 0.05, P95LatencyMS: 200,
		MeanCostUSD: 0.01, ApprovalRate: 0.95, IncidentCount: 0,
		OperatorRating: 4.0,
	}
	stable := base
	stable.RolloutState = "stable"
	canary := base
	canary.RolloutState = "canary"
	quarantine := base
	quarantine.RolloutState = "quarantine"
	deprecated := base
	deprecated.RolloutState = "deprecated"

	s := Score(stable, DefaultWeights())
	c := Score(canary, DefaultWeights())
	q := Score(quarantine, DefaultWeights())
	d := Score(deprecated, DefaultWeights())

	if !(s > c && c > q && q > d) {
		t.Fatalf("expected stable > canary > quarantine > deprecated, got %v %v %v %v", s, c, q, d)
	}
}

func TestScore_HigherSuccessRanksHigher(t *testing.T) {
	low := Metrics{SuccessRate: 0.5, ApprovalRate: 0.9, OperatorRating: 3, RolloutState: "stable"}
	high := Metrics{SuccessRate: 0.99, ApprovalRate: 0.9, OperatorRating: 3, RolloutState: "stable"}
	if Score(high, DefaultWeights()) <= Score(low, DefaultWeights()) {
		t.Fatalf("higher success should rank higher")
	}
}

func TestScore_ZeroWeightsFallsBackToDefault(t *testing.T) {
	m := Metrics{SuccessRate: 0.8, ApprovalRate: 0.8, OperatorRating: 4, RolloutState: "stable"}
	if Score(m, Weights{}) != Score(m, DefaultWeights()) {
		t.Fatalf("zero weights should fall back to DefaultWeights")
	}
}

func TestScore_StaysInUnitInterval(t *testing.T) {
	// Adversarial / out-of-range inputs must not escape [0,1].
	cases := []Metrics{
		{SuccessRate: 5, SchemaFailRate: -1, P95LatencyMS: -10, MeanCostUSD: -5, ApprovalRate: 9, OperatorRating: 99, IncidentCount: -3, RolloutState: "stable"},
		{SuccessRate: math.NaN(), RolloutState: "weird-state"},
		{IncidentCount: 1000000, RolloutState: "deprecated"},
	}
	for i, m := range cases {
		got := Score(m, DefaultWeights())
		if got < 0 || got > 1 || math.IsNaN(got) {
			t.Fatalf("case %d: score escaped [0,1]: %v", i, got)
		}
	}
}

func TestNormLatencyMonotonic(t *testing.T) {
	prev := normLatency(0)
	for ms := 100.0; ms <= 6000; ms += 100 {
		cur := normLatency(ms)
		if cur > prev {
			t.Fatalf("normLatency not monotonically decreasing at %v: %v > %v", ms, cur, prev)
		}
		prev = cur
	}
}

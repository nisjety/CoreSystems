// Package scoring computes a deterministic composite rank for a capability from
// its runtime health columns (success_rate, schema_fail_rate, p95_latency_ms,
// mean_cost_usd, approval_rate, incident_count, operator_rating) and lifecycle
// state (rollout_state).
//
// The score is a pure function of its inputs — no DB, no clock, no randomness —
// so it is fully unit-testable and produces a stable ordering the agentic loop
// can rely on when picking among interchangeable tools/capabilities.
//
// Scores are normalised to [0, 1]; higher is better. The weights below are the
// default registry policy and can be overridden per-call via Weights.
package scoring

import "math"

// Weights controls the relative contribution of each signal to the composite
// score. All fields are non-negative; the function normalises by their sum so
// the result stays in [0, 1] regardless of the absolute magnitudes chosen.
type Weights struct {
	Success    float64 // reward high success_rate
	SchemaFail float64 // penalise high schema_fail_rate
	Latency    float64 // penalise high p95 latency
	Cost       float64 // penalise high mean cost
	Approval   float64 // reward high approval_rate
	Incident   float64 // penalise incidents
	Operator   float64 // reward operator rating
}

// DefaultWeights is the registry's default scoring policy. Reliability and
// safety dominate; cost and latency are secondary tie-breakers.
func DefaultWeights() Weights {
	return Weights{
		Success:    0.30,
		SchemaFail: 0.15,
		Latency:    0.10,
		Cost:       0.10,
		Approval:   0.15,
		Incident:   0.10,
		Operator:   0.10,
	}
}

// Metrics is the subset of capability health columns the score consumes.
// Values use the same units as the capabilities table.
type Metrics struct {
	SuccessRate    float64 // [0,1]
	SchemaFailRate float64 // [0,1]
	P95LatencyMS   float64 // milliseconds, >= 0
	MeanCostUSD    float64 // USD per invocation, >= 0
	ApprovalRate   float64 // [0,1]
	IncidentCount  int     // count over the sample window, >= 0
	OperatorRating float64 // [0,5] human rating
	RolloutState   string  // canary | stable | quarantine | deprecated
}

// Normalisation reference points. A capability at or beyond the reference
// "bad" value contributes 0 to that term; at the "good" value it contributes 1.
const (
	// latencyFloorMS and latencyCeilMS bound the latency normalisation: <= floor
	// is ideal (1.0), >= ceil is worst (0.0).
	latencyFloorMS = 50.0
	latencyCeilMS  = 5000.0

	// costCeilUSD bounds cost normalisation: 0 cost is ideal (1.0), >= ceil is
	// worst (0.0).
	costCeilUSD = 1.0

	// incidentCeil bounds incident normalisation: 0 incidents is ideal (1.0),
	// >= ceil is worst (0.0).
	incidentCeil = 10.0

	// operatorMax is the top of the operator rating scale.
	operatorMax = 5.0
)

// rolloutMultiplier scales the final score by lifecycle state so a healthy but
// quarantined/deprecated capability never out-ranks a stable one. canary is
// mildly discounted; unknown states are treated as stable.
func rolloutMultiplier(state string) float64 {
	switch state {
	case "deprecated":
		return 0.10
	case "quarantine":
		return 0.25
	case "canary":
		return 0.85
	default: // stable / unknown
		return 1.0
	}
}

// clamp01 bounds x to [0,1].
func clamp01(x float64) float64 {
	if math.IsNaN(x) || x < 0 {
		return 0
	}
	if x > 1 {
		return 1
	}
	return x
}

// normLatency maps latency to [0,1] where lower latency scores higher.
func normLatency(ms float64) float64 {
	if ms <= latencyFloorMS {
		return 1
	}
	if ms >= latencyCeilMS {
		return 0
	}
	return 1 - (ms-latencyFloorMS)/(latencyCeilMS-latencyFloorMS)
}

// normCost maps cost to [0,1] where cheaper scores higher.
func normCost(usd float64) float64 {
	if usd <= 0 {
		return 1
	}
	if usd >= costCeilUSD {
		return 0
	}
	return 1 - usd/costCeilUSD
}

// normIncidents maps incident count to [0,1] where fewer scores higher.
func normIncidents(n int) float64 {
	if n <= 0 {
		return 1
	}
	f := float64(n)
	if f >= incidentCeil {
		return 0
	}
	return 1 - f/incidentCeil
}

// Score returns the composite rank in [0,1] for the given metrics under the
// supplied weights. A zero-value Weights (sum 0) falls back to DefaultWeights
// so callers can pass Weights{} to mean "use the default policy".
func Score(m Metrics, w Weights) float64 {
	sum := w.Success + w.SchemaFail + w.Latency + w.Cost + w.Approval + w.Incident + w.Operator
	if sum <= 0 {
		w = DefaultWeights()
		sum = w.Success + w.SchemaFail + w.Latency + w.Cost + w.Approval + w.Incident + w.Operator
	}

	terms := w.Success*clamp01(m.SuccessRate) +
		w.SchemaFail*clamp01(1-m.SchemaFailRate) +
		w.Latency*normLatency(m.P95LatencyMS) +
		w.Cost*normCost(m.MeanCostUSD) +
		w.Approval*clamp01(m.ApprovalRate) +
		w.Incident*normIncidents(m.IncidentCount) +
		w.Operator*clamp01(m.OperatorRating/operatorMax)

	base := terms / sum
	return clamp01(base * rolloutMultiplier(m.RolloutState))
}

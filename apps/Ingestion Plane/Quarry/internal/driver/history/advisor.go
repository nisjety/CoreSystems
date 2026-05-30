package history

import (
	"context"
	"sort"
)

const (
	// minSamples is the minimum number of recorded outcomes before the advisor
	// will deviate from the default engine ordering.
	minSamples = 5

	// minSuccessRateGain is how much better an alternative engine must be
	// (success rate delta) before it displaces the current front-runner.
	minSuccessRateGain = 0.05
)

// Recommendation holds the preferred engine order for a domain.
type Recommendation struct {
	// Engines is the ordered list of engine names, from most preferred to least.
	// May be empty when there is insufficient history.
	Engines []string
}

// Advisor uses the history Store to recommend which engine to try first for
// a given domain, replacing the static priority waterfall with a
// data-driven ordering.
type Advisor struct {
	store    *Store
	engines  []string // all known engine names, in default priority order
}

// NewAdvisor creates an Advisor. engines must be the full list of available
// engine names in their default priority order (lowest index = preferred).
func NewAdvisor(store *Store, engines []string) *Advisor {
	return &Advisor{store: store, engines: engines}
}

// Store returns the underlying history Store so callers can record outcomes.
func (a *Advisor) Store() *Store { return a.store }

// engineScore holds the computed score for one engine to enable sorting.
type engineScore struct {
	name        string
	successRate float64
	samples     int
	defaultIdx  int // position in the default priority list (lower = better)
}

// Recommend returns a reordered engine list for the domain.
// When there is insufficient history (< minSamples for any engine) the default
// ordering is returned unchanged so the caller can rely on stable behaviour
// even for freshly-seen domains.
func (a *Advisor) Recommend(ctx context.Context, domain string) Recommendation {
	scores := make([]engineScore, 0, len(a.engines))

	hasEnoughData := false
	for idx, eng := range a.engines {
		sr, _, n := a.store.Stats(ctx, domain, eng)
		if n >= minSamples {
			hasEnoughData = true
		}
		scores = append(scores, engineScore{
			name:        eng,
			successRate: sr,
			samples:     n,
			defaultIdx:  idx,
		})
	}

	if !hasEnoughData {
		// Not enough data yet — return default order.
		return Recommendation{Engines: a.engines}
	}

	// Sort: prefer engines with higher success rates. Break ties by default
	// priority index (lower index = preferred by default).
	sort.SliceStable(scores, func(i, j int) bool {
		si, sj := scores[i], scores[j]
		// If the difference is below the minimum gain threshold, prefer the
		// default-order engine to avoid unnecessary churn.
		if sj.samples >= minSamples && si.successRate-sj.successRate < minSuccessRateGain {
			return si.defaultIdx < sj.defaultIdx
		}
		return si.successRate > sj.successRate
	})

	ordered := make([]string, len(scores))
	for i, s := range scores {
		ordered[i] = s.name
	}
	return Recommendation{Engines: ordered}
}

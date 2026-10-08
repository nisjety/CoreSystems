package ledger

import (
	"context"
	"errors"
	"sort"
)

// Buckets contain accounting dimensions only, never prompts or response content.
// Limit groups rather than source events: a truncated report must not look complete.
const MaxAnalyticsBuckets = 10000

var ErrAnalyticsTooLarge = errors.New("usage report is too large; select a shorter period")

type UsageBucket struct {
	Day                string  `json:"day"`
	Model              string  `json:"model"`
	Provider           string  `json:"provider"`
	Basis              string  `json:"basis"`
	Requests           int64   `json:"requests"`
	InputTokens        int64   `json:"input_tokens"`
	OutputTokens       int64   `json:"output_tokens"`
	MeteredCostUSD     float64 `json:"metered_cost_usd"`
	UnmeasuredRequests int64   `json:"unmeasured_requests"`
}

type AnalyticsReader interface {
	UsageAnalytics(context.Context, AggregateFilter) ([]UsageBucket, error)
}

func (s *Store) UsageAnalytics(_ context.Context, f AggregateFilter) ([]UsageBucket, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	buckets := map[[4]string]*UsageBucket{}
	for _, e := range s.entries {
		if !matches(e, f) {
			continue
		}
		basis := e.CostBasis
		if basis == "" {
			basis = "unknown"
		}
		key := [4]string{e.CreatedAt.UTC().Format("2006-01-02"), e.Model, e.ProviderUsed, basis}
		b := buckets[key]
		if b == nil {
			b = &UsageBucket{Day: key[0], Model: key[1], Provider: key[2], Basis: key[3]}
			buckets[key] = b
			if len(buckets) > MaxAnalyticsBuckets {
				return nil, ErrAnalyticsTooLarge
			}
		}
		b.Requests++
		b.InputTokens += e.InputTokens
		b.OutputTokens += e.OutputTokens
		b.MeteredCostUSD += basisCost(e, CostBasisMetered)
		b.UnmeasuredRequests += zeroTokenCount(e)
	}
	out := make([]UsageBucket, 0, len(buckets))
	for _, b := range buckets {
		out = append(out, *b)
	}
	sort.Slice(out, func(i, j int) bool {
		a, b := out[i], out[j]
		if a.Day != b.Day {
			return a.Day < b.Day
		}
		if a.Model != b.Model {
			return a.Model < b.Model
		}
		if a.Provider != b.Provider {
			return a.Provider < b.Provider
		}
		return a.Basis < b.Basis
	})
	return out, nil
}

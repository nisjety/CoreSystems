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
	// The cache fields cover metered and unknown-basis entries only: a
	// subscription turn has no cache spend to report. CacheReportedInputTokens
	// is the input of the entries where the provider showed cache activity (a
	// read, a write, or a classified miss other than below_minimum), so
	// read / reported is the hit rate and a bucket with 0 reported has none.
	CacheReadInputTokens     int64 `json:"cache_read_input_tokens"`
	CacheCreationInputTokens int64 `json:"cache_creation_input_tokens"`
	CacheReportedInputTokens int64 `json:"cache_reported_input_tokens"`
}

// CacheMissBucket counts the classified cache misses of one day, model and
// provider by advisory reason.
type CacheMissBucket struct {
	Day      string `json:"day"`
	Model    string `json:"model"`
	Provider string `json:"provider"`
	Reason   string `json:"reason"`
	Requests int64  `json:"requests"`
}

type AnalyticsReader interface {
	UsageAnalytics(context.Context, AggregateFilter) ([]UsageBucket, error)
}

// CacheMissReader is the optional second read behind the analytics
// cache_breakdown.
type CacheMissReader interface {
	CacheMissAnalytics(context.Context, AggregateFilter) ([]CacheMissBucket, error)
}

// cacheReported says whether the provider showed cache activity on the entry.
// A below_minimum miss is not activity: the prompt was too short to cache.
func cacheReported(e Entry) bool {
	return e.CacheReadInputTokens > 0 || e.CacheCreationInputTokens > 0 ||
		(e.CacheMissReason != "" && e.CacheMissReason != CacheMissBelowMinimum)
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
		if e.CostBasis != CostBasisSubscription {
			b.CacheReadInputTokens += e.CacheReadInputTokens
			b.CacheCreationInputTokens += e.CacheCreationInputTokens
			if cacheReported(e) {
				b.CacheReportedInputTokens += e.InputTokens
			}
		}
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

func (s *Store) CacheMissAnalytics(_ context.Context, f AggregateFilter) ([]CacheMissBucket, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	buckets := map[[4]string]*CacheMissBucket{}
	for _, e := range s.entries {
		if !matches(e, f) || e.CacheMissReason == "" || e.CostBasis == CostBasisSubscription {
			continue
		}
		key := [4]string{e.CreatedAt.UTC().Format("2006-01-02"), e.Model, e.ProviderUsed, e.CacheMissReason}
		b := buckets[key]
		if b == nil {
			b = &CacheMissBucket{Day: key[0], Model: key[1], Provider: key[2], Reason: key[3]}
			buckets[key] = b
			if len(buckets) > MaxAnalyticsBuckets {
				return nil, ErrAnalyticsTooLarge
			}
		}
		b.Requests++
	}
	out := make([]CacheMissBucket, 0, len(buckets))
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
		return a.Reason < b.Reason
	})
	return out, nil
}

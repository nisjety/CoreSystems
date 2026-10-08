package ledger

import (
	"context"
	"sort"
)

// UserUsage deliberately excludes request/run IDs and content. A blank user
// is a producer-only record, never silently charged to the current user.
type UserUsage struct {
	UserID                 string  `json:"user_id"`
	EntryCount             int64   `json:"entry_count"`
	InputTokens            int64   `json:"input_tokens"`
	OutputTokens           int64   `json:"output_tokens"`
	MeteredCostUSD         float64 `json:"metered_cost_usd"`
	MeteredEntries         int64   `json:"metered_entries"`
	SubscriptionEntries    int64   `json:"subscription_entries"`
	UnknownBasisEntries    int64   `json:"unknown_basis_entries"`
	UnmeasuredTokenEntries int64   `json:"unmeasured_token_entries"`
}

type UserUsageReader interface {
	AggregateUsers(context.Context, AggregateFilter) ([]UserUsage, error)
}

func (s *Store) AggregateUsers(_ context.Context, f AggregateFilter) ([]UserUsage, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	users := map[string]*UserUsage{}
	for _, e := range s.entries {
		if !matches(e, f) {
			continue
		}
		u := users[e.UserID]
		if u == nil {
			u = &UserUsage{UserID: e.UserID}
			users[e.UserID] = u
		}
		u.EntryCount++
		u.InputTokens += e.InputTokens
		u.OutputTokens += e.OutputTokens
		u.MeteredCostUSD += basisCost(e, CostBasisMetered)
		u.MeteredEntries += basisCount(e.CostBasis, CostBasisMetered)
		u.SubscriptionEntries += basisCount(e.CostBasis, CostBasisSubscription)
		u.UnknownBasisEntries += basisCount(e.CostBasis, CostBasisUnknown)
		u.UnmeasuredTokenEntries += zeroTokenCount(e)
	}
	out := make([]UserUsage, 0, len(users))
	for _, u := range users {
		out = append(out, *u)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].UserID < out[j].UserID })
	return out, nil
}

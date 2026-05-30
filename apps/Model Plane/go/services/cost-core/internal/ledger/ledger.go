// Package ledger implements an in-memory token/cost ledger for tracking
// per-org, per-user usage and enforcing budget caps.
package ledger

import (
	"errors"
	"fmt"
	"sync"
)

var (
	// ErrBudgetExceededCost indicates the cumulative cost exceeds the
	// configured USD cap.
	ErrBudgetExceededCost = errors.New("budget exceeded: max cost USD")
	// ErrBudgetExceededTokens indicates the cumulative token count exceeds
	// the configured token cap.
	ErrBudgetExceededTokens = errors.New("budget exceeded: max tokens")
	// ErrUsageNotFound indicates no usage record exists for the given key.
	ErrUsageNotFound = errors.New("usage not found")
)

// Usage holds accumulated token and cost figures for a single org+user key.
type Usage struct {
	OrgID             string
	UserID            string
	TotalInputTokens  int64
	TotalOutputTokens int64
	TotalCostUSD      float64
}

// key is the composite lookup key for the ledger map.
type key struct {
	OrgID  string
	UserID string
}

// Store is a goroutine-safe in-memory ledger.
type Store struct {
	mu    sync.Mutex
	usage map[key]*Usage
}

// NewStore constructs an empty Store.
func NewStore() *Store {
	return &Store{usage: make(map[key]*Usage)}
}

// Record adds a usage event to the ledger for the given org and user.
// inputTokens and outputTokens are additive; costUSD is additive.
func (s *Store) Record(orgID, userID string, inputTokens, outputTokens int32, costUSD float64) {
	k := key{OrgID: orgID, UserID: userID}

	s.mu.Lock()
	defer s.mu.Unlock()

	u, ok := s.usage[k]
	if !ok {
		u = &Usage{OrgID: orgID, UserID: userID}
		s.usage[k] = u
	}
	u.TotalInputTokens += int64(inputTokens)
	u.TotalOutputTokens += int64(outputTokens)
	u.TotalCostUSD += costUSD
}

// GetUsage returns a snapshot of the current usage for the given org and user.
// Returns ErrUsageNotFound if no record exists.
func (s *Store) GetUsage(orgID, userID string) (*Usage, error) {
	k := key{OrgID: orgID, UserID: userID}

	s.mu.Lock()
	defer s.mu.Unlock()

	u, ok := s.usage[k]
	if !ok {
		return nil, ErrUsageNotFound
	}

	// Return a copy to avoid callers mutating the internal state.
	snapshot := *u
	return &snapshot, nil
}

// CheckBudget verifies that the accumulated usage for the given org and user
// does not exceed the provided caps. Pass maxCostUSD <= 0 or maxTokens <= 0
// to skip that specific check.
//
// Returns nil if within budget, or a descriptive error if either cap is
// exceeded.
func (s *Store) CheckBudget(orgID, userID string, maxCostUSD float64, maxTokens int64) error {
	k := key{OrgID: orgID, UserID: userID}

	s.mu.Lock()
	defer s.mu.Unlock()

	u, ok := s.usage[k]
	if !ok {
		// No usage recorded yet — always within budget.
		return nil
	}

	if maxCostUSD > 0 && u.TotalCostUSD >= maxCostUSD {
		return fmt.Errorf("%w: current %.6f >= limit %.6f", ErrBudgetExceededCost, u.TotalCostUSD, maxCostUSD)
	}

	totalTokens := u.TotalInputTokens + u.TotalOutputTokens
	if maxTokens > 0 && totalTokens >= maxTokens {
		return fmt.Errorf("%w: current %d >= limit %d", ErrBudgetExceededTokens, totalTokens, maxTokens)
	}

	return nil
}

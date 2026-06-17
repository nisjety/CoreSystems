// Package ledger implements the cost-core token/cost ledger: an append-only
// record of cost-bearing events plus the rollup/aggregation queries that the
// budget guard and runs/billing surfaces read.
//
// Two implementations satisfy the [Ledger] interface:
//   - [Store]      — in-memory, used as a fallback when no DATABASE_URL is set
//     (local dev, tests). It keeps both per-key rollups and the raw entries so
//     it can answer the same queries as the durable store.
//   - postgres.Store (internal/postgres) — the durable, Postgres-backed store.
package ledger

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"sync"
	"time"
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

// Entry is a single cost-bearing event written to the ledger.
type Entry struct {
	OrgID          string
	UserID         string
	RunID          string
	RequestID      string
	Model          string
	InputTokens    int64
	OutputTokens   int64
	CostUSD        float64
	IdempotencyKey string
	CreatedAt      time.Time
}

// Usage holds accumulated token and cost figures for a single aggregation key.
type Usage struct {
	OrgID             string
	UserID            string
	RunID             string
	TotalInputTokens  int64
	TotalOutputTokens int64
	TotalCostUSD      float64
	EntryCount        int64
}

// AggregateFilter narrows an Aggregate/List query. Empty string fields are
// ignored (not used as a predicate); the zero time bounds are ignored too.
type AggregateFilter struct {
	OrgID  string
	UserID string
	RunID  string
	Model  string
	Since  time.Time
	Until  time.Time
}

// Ledger is the durable cost-ledger contract shared by the in-memory and
// Postgres implementations.
type Ledger interface {
	// RecordEntry appends a cost event. When IdempotencyKey is non-empty the
	// same key is recorded at most once (later duplicates are ignored). The
	// CreatedAt field is set by the store if zero.
	RecordEntry(ctx context.Context, e Entry) error

	// GetUsage returns the rolled-up totals for an org+user pair. Returns
	// ErrUsageNotFound when no entries exist for the key.
	GetUsage(ctx context.Context, orgID, userID string) (*Usage, error)

	// GetRunUsage returns the rolled-up totals for a single run. Returns
	// ErrUsageNotFound when no entries exist for the run.
	GetRunUsage(ctx context.Context, runID string) (*Usage, error)

	// Aggregate returns the rolled-up totals across all entries matching the
	// filter. It never returns ErrUsageNotFound: an empty result is a
	// zero-valued Usage scoped to the filter's org/user/run.
	Aggregate(ctx context.Context, f AggregateFilter) (*Usage, error)

	// ListEntries returns the most recent entries matching the filter, newest
	// first, capped at limit (a limit <= 0 applies a default).
	ListEntries(ctx context.Context, f AggregateFilter, limit int) ([]Entry, error)

	// CheckBudget returns nil when the org+user is within both caps, or an
	// ErrBudgetExceeded* error otherwise. A cap <= 0 disables that check.
	CheckBudget(ctx context.Context, orgID, userID string, maxCostUSD float64, maxTokens int64) error
}

const defaultListLimit = 100

// key is the composite lookup key for the in-memory rollup map.
type key struct {
	OrgID  string
	UserID string
}

// Store is a goroutine-safe in-memory [Ledger] used as a fallback.
type Store struct {
	mu      sync.Mutex
	usage   map[key]*Usage
	entries []Entry
	seen    map[string]struct{} // idempotency keys already recorded
}

// NewStore constructs an empty in-memory Store.
func NewStore() *Store {
	return &Store{
		usage: make(map[key]*Usage),
		seen:  make(map[string]struct{}),
	}
}

// Ensure Store satisfies the Ledger interface.
var _ Ledger = (*Store)(nil)

// RecordEntry appends a cost event to the in-memory ledger.
func (s *Store) RecordEntry(_ context.Context, e Entry) error {
	if e.CreatedAt.IsZero() {
		e.CreatedAt = time.Now().UTC()
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	if e.IdempotencyKey != "" {
		if _, ok := s.seen[e.IdempotencyKey]; ok {
			return nil // duplicate — already recorded
		}
		s.seen[e.IdempotencyKey] = struct{}{}
	}

	s.entries = append(s.entries, e)

	k := key{OrgID: e.OrgID, UserID: e.UserID}
	u, ok := s.usage[k]
	if !ok {
		u = &Usage{OrgID: e.OrgID, UserID: e.UserID}
		s.usage[k] = u
	}
	u.TotalInputTokens += e.InputTokens
	u.TotalOutputTokens += e.OutputTokens
	u.TotalCostUSD += e.CostUSD
	u.EntryCount++
	return nil
}

// GetUsage returns a snapshot of the rolled-up usage for the given org+user.
func (s *Store) GetUsage(_ context.Context, orgID, userID string) (*Usage, error) {
	k := key{OrgID: orgID, UserID: userID}

	s.mu.Lock()
	defer s.mu.Unlock()

	u, ok := s.usage[k]
	if !ok {
		return nil, ErrUsageNotFound
	}
	snapshot := *u
	return &snapshot, nil
}

// GetRunUsage rolls up all entries for a single run.
func (s *Store) GetRunUsage(_ context.Context, runID string) (*Usage, error) {
	if runID == "" {
		return nil, ErrUsageNotFound
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	out := &Usage{RunID: runID}
	found := false
	for i := range s.entries {
		e := s.entries[i]
		if e.RunID != runID {
			continue
		}
		found = true
		out.OrgID = e.OrgID
		out.TotalInputTokens += e.InputTokens
		out.TotalOutputTokens += e.OutputTokens
		out.TotalCostUSD += e.CostUSD
		out.EntryCount++
	}
	if !found {
		return nil, ErrUsageNotFound
	}
	return out, nil
}

// Aggregate rolls up all entries matching the filter.
func (s *Store) Aggregate(_ context.Context, f AggregateFilter) (*Usage, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	out := &Usage{OrgID: f.OrgID, UserID: f.UserID, RunID: f.RunID}
	for i := range s.entries {
		if !matches(s.entries[i], f) {
			continue
		}
		e := s.entries[i]
		out.TotalInputTokens += e.InputTokens
		out.TotalOutputTokens += e.OutputTokens
		out.TotalCostUSD += e.CostUSD
		out.EntryCount++
	}
	return out, nil
}

// ListEntries returns the most recent matching entries, newest first.
func (s *Store) ListEntries(_ context.Context, f AggregateFilter, limit int) ([]Entry, error) {
	if limit <= 0 {
		limit = defaultListLimit
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	var out []Entry
	for i := range s.entries {
		if matches(s.entries[i], f) {
			out = append(out, s.entries[i])
		}
	}
	sort.SliceStable(out, func(i, j int) bool {
		return out[i].CreatedAt.After(out[j].CreatedAt)
	})
	if len(out) > limit {
		out = out[:limit]
	}
	return out, nil
}

// CheckBudget verifies the accumulated org+user usage against the caps.
func (s *Store) CheckBudget(_ context.Context, orgID, userID string, maxCostUSD float64, maxTokens int64) error {
	k := key{OrgID: orgID, UserID: userID}

	s.mu.Lock()
	defer s.mu.Unlock()

	u, ok := s.usage[k]
	if !ok {
		return nil // no usage yet — within budget
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

// matches reports whether entry e satisfies filter f.
func matches(e Entry, f AggregateFilter) bool {
	if f.OrgID != "" && e.OrgID != f.OrgID {
		return false
	}
	if f.UserID != "" && e.UserID != f.UserID {
		return false
	}
	if f.RunID != "" && e.RunID != f.RunID {
		return false
	}
	if f.Model != "" && e.Model != f.Model {
		return false
	}
	if !f.Since.IsZero() && e.CreatedAt.Before(f.Since) {
		return false
	}
	if !f.Until.IsZero() && e.CreatedAt.After(f.Until) {
		return false
	}
	return true
}

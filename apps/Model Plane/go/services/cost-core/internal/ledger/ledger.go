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
	"math"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"
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
	// ErrInvalidEntry indicates a cost event cannot safely affect accounting.
	ErrInvalidEntry = errors.New("invalid accounting entry")
)

const (
	// MaxTokensPerEntry is deliberately far above current provider limits while
	// bounding sums and rejecting accidental int overflows at the boundary.
	MaxTokensPerEntry int64 = 1_000_000_000_000
	// MaxCostUSD matches the integer capacity of numeric(20,10) in Postgres.
	MaxCostUSD float64 = 9_999_999_999
	// MaxDimensionBytes bounds tenant, actor, run, request, model, and producer IDs.
	MaxDimensionBytes = 256
	// MaxIdempotencyKeyBytes permits composite upstream keys without unbounded indexes.
	MaxIdempotencyKeyBytes = 512
)

// Cost bases an entry may carry: how its cost_usd was established. They are
// the wire values of model-gateway's `pricing::CostBasis` on the
// USAGE_ENVELOPE payload's `cost_basis` field.
const (
	// CostBasisUnknown is an entry whose producer did not say, such as a
	// gateway build older than the field. It is priced like a metered entry
	// and persisted as NULL.
	CostBasisUnknown = ""
	// CostBasisMetered is an API route, priced per token from the catalogue.
	CostBasisMetered = "metered"
	// CostBasisSubscription is a call served by the user's own ChatGPT
	// subscription. It has no per-token charge: its cost stays 0 and its
	// token counts are usage, not spend. It is never priced from the
	// catalogue.
	CostBasisSubscription = "subscription"
)

// NormalizeCostBasis maps a reported cost basis to its canonical value.
// Surrounding space and case are ignored. ok is false for a non-empty value
// that is not a known basis; the returned basis is then CostBasisUnknown.
func NormalizeCostBasis(raw string) (basis string, ok bool) {
	switch value := strings.ToLower(strings.TrimSpace(raw)); value {
	case CostBasisUnknown, CostBasisMetered, CostBasisSubscription:
		return value, true
	default:
		return CostBasisUnknown, false
	}
}

// Entry is a single cost-bearing event written to the ledger.
type Entry struct {
	OrgID          string
	UserID         string
	ProducerID     string
	RunID          string
	RequestID      string
	Model          string
	ProviderUsed   string
	InputTokens    int64
	OutputTokens   int64
	CostUSD        float64
	IdempotencyKey string
	CreatedAt      time.Time
	// CacheReadInputTokens and CacheCreationInputTokens are prompt-cache
	// telemetry (a native-compaction migration prerequisite): tokens served
	// from / newly written to the serving provider's prompt cache. Both are
	// already folded into InputTokens by the serving adapter; carried here
	// separately so cache-hit rate and cost savings are queryable off the
	// durable ledger. 0 for an event that reports no cache usage -- always
	// present, never a nullable column a reader must existence-check.
	CacheReadInputTokens     int64
	CacheCreationInputTokens int64
	// CostBasis says how CostUSD was established: one of the CostBasis*
	// constants. A CostBasisSubscription entry keeps its real token counts
	// with a cost of 0; the other bases are priced from the catalogue when
	// they arrive without a cost.
	CostBasis string
}

// ValidateEntry rejects values that can poison aggregates, overflow durable
// columns, or lose canonical attribution. At least one user or service
// producer identity must be present; authentication happens at the transport
// boundary before this storage-level validation.
func ValidateEntry(e Entry) error {
	if err := ValidateScope(e.OrgID, e.UserID); err != nil {
		return err
	}
	for _, field := range []struct {
		name     string
		value    string
		required bool
		limit    int
	}{
		{name: "producer_id", value: e.ProducerID, limit: MaxDimensionBytes},
		{name: "run_id", value: e.RunID, limit: MaxDimensionBytes},
		{name: "request_id", value: e.RequestID, limit: MaxDimensionBytes},
		{name: "model", value: e.Model, limit: MaxDimensionBytes},
		{name: "provider_used", value: e.ProviderUsed, limit: MaxDimensionBytes},
		{name: "idempotency_key", value: e.IdempotencyKey, limit: MaxIdempotencyKeyBytes},
	} {
		if err := validateString(field.name, field.value, field.required, field.limit); err != nil {
			return err
		}
	}
	if e.UserID == "" && e.ProducerID == "" {
		return fmt.Errorf("%w: user_id or producer_id is required", ErrInvalidEntry)
	}
	if e.InputTokens < 0 || e.OutputTokens < 0 ||
		e.InputTokens > MaxTokensPerEntry || e.OutputTokens > MaxTokensPerEntry ||
		e.InputTokens > MaxTokensPerEntry-e.OutputTokens {
		return fmt.Errorf("%w: token counts must be nonnegative and total at most %d", ErrInvalidEntry, MaxTokensPerEntry)
	}
	if e.CacheReadInputTokens < 0 || e.CacheCreationInputTokens < 0 ||
		e.CacheReadInputTokens > MaxTokensPerEntry || e.CacheCreationInputTokens > MaxTokensPerEntry {
		return fmt.Errorf("%w: cache token counts must be nonnegative and at most %d", ErrInvalidEntry, MaxTokensPerEntry)
	}
	if math.IsNaN(e.CostUSD) || math.IsInf(e.CostUSD, 0) || e.CostUSD < 0 || e.CostUSD > MaxCostUSD {
		return fmt.Errorf("%w: cost_usd must be finite and between 0 and %.0f", ErrInvalidEntry, MaxCostUSD)
	}
	if basis, ok := NormalizeCostBasis(e.CostBasis); !ok || basis != e.CostBasis {
		return fmt.Errorf("%w: cost_basis must be empty, %q or %q", ErrInvalidEntry, CostBasisMetered, CostBasisSubscription)
	}
	return nil
}

// ValidateScope verifies canonical tenant and optional user identifiers.
func ValidateScope(orgID, userID string) error {
	if err := validateString("org_id", orgID, true, MaxDimensionBytes); err != nil {
		return err
	}
	return validateString("user_id", userID, false, MaxDimensionBytes)
}

// ValidateBudget rejects malformed caps. Zero disables a cap; negative,
// non-finite, or implausibly large caps are caller errors rather than opt-outs.
func ValidateBudget(maxCostUSD float64, maxTokens int64) error {
	if math.IsNaN(maxCostUSD) || math.IsInf(maxCostUSD, 0) || maxCostUSD < 0 || maxCostUSD > MaxCostUSD {
		return fmt.Errorf("%w: max_cost_usd is outside the supported range", ErrInvalidEntry)
	}
	if maxTokens < 0 || maxTokens > MaxTokensPerEntry {
		return fmt.Errorf("%w: max_tokens is outside the supported range", ErrInvalidEntry)
	}
	return nil
}

func validateString(name, value string, required bool, limit int) error {
	if required && value == "" {
		return fmt.Errorf("%w: %s is required", ErrInvalidEntry, name)
	}
	if value == "" {
		return nil
	}
	if !utf8.ValidString(value) || value != strings.TrimSpace(value) || len(value) > limit {
		return fmt.Errorf("%w: %s is not canonical or exceeds %d bytes", ErrInvalidEntry, name, limit)
	}
	for _, r := range value {
		if unicode.IsControl(r) {
			return fmt.Errorf("%w: %s contains control characters", ErrInvalidEntry, name)
		}
	}
	return nil
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
	// TotalCacheReadInputTokens and TotalCacheCreationInputTokens roll up
	// Entry.CacheReadInputTokens / Entry.CacheCreationInputTokens the same
	// way TotalInputTokens rolls up InputTokens. Already included in
	// TotalInputTokens; broken out for cache-hit-rate reporting.
	TotalCacheReadInputTokens     int64
	TotalCacheCreationInputTokens int64
	// MeteredInputTokens and MeteredOutputTokens roll up the tokens of every
	// entry that is not CostBasisSubscription (an unreported basis counts as
	// metered). Token budgets cap these: a subscription call is usage, not
	// spend, so it never exhausts max_tokens. The Total* fields still report
	// every entry.
	MeteredInputTokens  int64
	MeteredOutputTokens int64
	// Cost basis is kept separate from token-budget totals so reports do not
	// mistake legacy or unknown entries for metered API usage.
	MeteredEntryCount        int64
	SubscriptionEntryCount   int64
	UnknownBasisEntryCount   int64
	MeteredCostUSD           float64
	SubscriptionInputTokens  int64
	SubscriptionOutputTokens int64
	UnknownBasisInputTokens  int64
	UnknownBasisOutputTokens int64
	ZeroTokenEntryCount      int64
	LatestEntryAt            time.Time
}

// meteredTokens is the part of e's token counts that a token budget caps.
func meteredTokens(e Entry) (input, output int64) {
	if e.CostBasis == CostBasisSubscription {
		return 0, 0
	}
	return e.InputTokens, e.OutputTokens
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

	// PurgeOrg permanently deletes every cost/usage row scoped to orgID. Used
	// by the GDPR org-erasure consumer (internal/consumers/org_erasure_consumer.go)
	// when org-core publishes verevon.gdpr.erasure.requested for
	// subject_type=="organization". Idempotent: calling it twice for the same
	// orgID is safe (the second call deletes zero rows). Must never affect any
	// other org's rows.
	PurgeOrg(ctx context.Context, orgID string) error
}

const defaultListLimit = 100

// key is the composite lookup key for the in-memory rollup map.
type key struct {
	OrgID  string
	UserID string
}

// Store is a goroutine-safe in-memory [Ledger] used as a fallback.
type Store struct {
	erased  map[string]struct{}
	mu      sync.Mutex
	usage   map[key]*Usage
	entries []Entry
	seen    map[idempotencyKey]struct{} // tenant-scoped idempotency keys already recorded
}

type idempotencyKey struct {
	OrgID string
	Key   string
}

// NewStore constructs an empty in-memory Store.
func NewStore() *Store {
	return &Store{
		erased: make(map[string]struct{}),
		usage:  make(map[key]*Usage),
		seen:   make(map[idempotencyKey]struct{}),
	}
}

// Ensure Store satisfies the Ledger interface.
var _ Ledger = (*Store)(nil)

// RecordEntry appends a cost event to the in-memory ledger.
func (s *Store) RecordEntry(_ context.Context, e Entry) error {
	if err := ValidateEntry(e); err != nil {
		return err
	}
	if e.CreatedAt.IsZero() {
		e.CreatedAt = time.Now().UTC()
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	if _, erased := s.erased[ErasedOrgHash(e.OrgID)]; erased {
		return ErrOrgErased
	}

	if e.IdempotencyKey != "" {
		idempotency := idempotencyKey{OrgID: e.OrgID, Key: e.IdempotencyKey}
		if _, ok := s.seen[idempotency]; ok {
			return nil // duplicate — already recorded
		}
	}

	k := key{OrgID: e.OrgID, UserID: e.UserID}
	u, ok := s.usage[k]
	if !ok {
		u = &Usage{OrgID: e.OrgID, UserID: e.UserID}
	}
	if e.InputTokens > math.MaxInt64-u.TotalInputTokens ||
		e.OutputTokens > math.MaxInt64-u.TotalOutputTokens ||
		u.EntryCount == math.MaxInt64 || math.IsInf(u.TotalCostUSD+e.CostUSD, 0) {
		return fmt.Errorf("%w: aggregate would overflow", ErrInvalidEntry)
	}
	meteredInput, meteredOutput := meteredTokens(e)
	next := &Usage{
		OrgID:                         u.OrgID,
		UserID:                        u.UserID,
		RunID:                         u.RunID,
		TotalInputTokens:              u.TotalInputTokens + e.InputTokens,
		TotalOutputTokens:             u.TotalOutputTokens + e.OutputTokens,
		TotalCostUSD:                  u.TotalCostUSD + e.CostUSD,
		EntryCount:                    u.EntryCount + 1,
		TotalCacheReadInputTokens:     u.TotalCacheReadInputTokens + e.CacheReadInputTokens,
		TotalCacheCreationInputTokens: u.TotalCacheCreationInputTokens + e.CacheCreationInputTokens,
		MeteredInputTokens:            u.MeteredInputTokens + meteredInput,
		MeteredOutputTokens:           u.MeteredOutputTokens + meteredOutput,
		MeteredEntryCount:             u.MeteredEntryCount + basisCount(e.CostBasis, CostBasisMetered),
		SubscriptionEntryCount:        u.SubscriptionEntryCount + basisCount(e.CostBasis, CostBasisSubscription),
		UnknownBasisEntryCount:        u.UnknownBasisEntryCount + basisCount(e.CostBasis, CostBasisUnknown),
		MeteredCostUSD:                u.MeteredCostUSD + basisCost(e, CostBasisMetered),
		SubscriptionInputTokens:       u.SubscriptionInputTokens + basisTokens(e, CostBasisSubscription, true),
		SubscriptionOutputTokens:      u.SubscriptionOutputTokens + basisTokens(e, CostBasisSubscription, false),
		UnknownBasisInputTokens:       u.UnknownBasisInputTokens + basisTokens(e, CostBasisUnknown, true),
		UnknownBasisOutputTokens:      u.UnknownBasisOutputTokens + basisTokens(e, CostBasisUnknown, false),
		ZeroTokenEntryCount:           u.ZeroTokenEntryCount + zeroTokenCount(e),
		LatestEntryAt:                 e.CreatedAt,
	}
	if u.LatestEntryAt.After(e.CreatedAt) {
		next.LatestEntryAt = u.LatestEntryAt
	}
	s.usage[k] = next
	s.entries = append(s.entries, e)
	if e.IdempotencyKey != "" {
		s.seen[idempotencyKey{OrgID: e.OrgID, Key: e.IdempotencyKey}] = struct{}{}
	}
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
		out.TotalCacheReadInputTokens += e.CacheReadInputTokens
		out.TotalCacheCreationInputTokens += e.CacheCreationInputTokens
		out.MeteredEntryCount += basisCount(e.CostBasis, CostBasisMetered)
		out.SubscriptionEntryCount += basisCount(e.CostBasis, CostBasisSubscription)
		out.UnknownBasisEntryCount += basisCount(e.CostBasis, CostBasisUnknown)
		out.MeteredCostUSD += basisCost(e, CostBasisMetered)
		out.SubscriptionInputTokens += basisTokens(e, CostBasisSubscription, true)
		out.SubscriptionOutputTokens += basisTokens(e, CostBasisSubscription, false)
		out.UnknownBasisInputTokens += basisTokens(e, CostBasisUnknown, true)
		out.UnknownBasisOutputTokens += basisTokens(e, CostBasisUnknown, false)
		out.ZeroTokenEntryCount += zeroTokenCount(e)
		if out.LatestEntryAt.IsZero() || e.CreatedAt.After(out.LatestEntryAt) {
			out.LatestEntryAt = e.CreatedAt
		}
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
		out.TotalCacheReadInputTokens += e.CacheReadInputTokens
		out.TotalCacheCreationInputTokens += e.CacheCreationInputTokens
		meteredInput, meteredOutput := meteredTokens(e)
		out.MeteredInputTokens += meteredInput
		out.MeteredOutputTokens += meteredOutput
		out.MeteredEntryCount += basisCount(e.CostBasis, CostBasisMetered)
		out.SubscriptionEntryCount += basisCount(e.CostBasis, CostBasisSubscription)
		out.UnknownBasisEntryCount += basisCount(e.CostBasis, CostBasisUnknown)
		out.MeteredCostUSD += basisCost(e, CostBasisMetered)
		out.SubscriptionInputTokens += basisTokens(e, CostBasisSubscription, true)
		out.SubscriptionOutputTokens += basisTokens(e, CostBasisSubscription, false)
		out.UnknownBasisInputTokens += basisTokens(e, CostBasisUnknown, true)
		out.UnknownBasisOutputTokens += basisTokens(e, CostBasisUnknown, false)
		out.ZeroTokenEntryCount += zeroTokenCount(e)
		if out.LatestEntryAt.IsZero() || e.CreatedAt.After(out.LatestEntryAt) {
			out.LatestEntryAt = e.CreatedAt
		}
	}
	return out, nil
}

func basisCount(raw, basis string) int64 {
	if raw == basis {
		return 1
	}
	return 0
}

func basisCost(entry Entry, basis string) float64 {
	if entry.CostBasis == basis {
		return entry.CostUSD
	}
	return 0
}

func basisTokens(entry Entry, basis string, input bool) int64 {
	if entry.CostBasis != basis {
		return 0
	}
	if input {
		return entry.InputTokens
	}
	return entry.OutputTokens
}

func zeroTokenCount(entry Entry) int64 {
	if entry.InputTokens == 0 && entry.OutputTokens == 0 {
		return 1
	}
	return 0
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
	if err := ValidateScope(orgID, userID); err != nil {
		return err
	}
	if err := ValidateBudget(maxCostUSD, maxTokens); err != nil {
		return err
	}
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
	totalTokens := u.MeteredInputTokens + u.MeteredOutputTokens
	if maxTokens > 0 && totalTokens >= maxTokens {
		return fmt.Errorf("%w: current %d >= limit %d", ErrBudgetExceededTokens, totalTokens, maxTokens)
	}
	return nil
}

// PurgeOrg deletes every in-memory usage rollup, raw entry, and idempotency
// record for orgID. Scoped strictly by orgID so other orgs are never touched.
func (s *Store) PurgeOrg(_ context.Context, orgID string) error {
	if err := ValidateScope(orgID, ""); err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.erased == nil {
		s.erased = make(map[string]struct{})
	}
	s.erased[ErasedOrgHash(orgID)] = struct{}{}

	for k := range s.usage {
		if k.OrgID == orgID {
			delete(s.usage, k)
		}
	}

	kept := s.entries[:0:0]
	for _, e := range s.entries {
		if e.OrgID != orgID {
			kept = append(kept, e)
		}
	}
	s.entries = kept

	for k := range s.seen {
		if k.OrgID == orgID {
			delete(s.seen, k)
		}
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

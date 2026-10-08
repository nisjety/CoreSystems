// Package postgres provides the durable, Postgres-backed implementation of the
// cost-core ledger ([ledger.Ledger]). Cost-bearing events are appended to the
// cost_entries table; rollups are computed with SQL aggregates at query time
// so detail is never lost and totals can always be re-derived.
package postgres

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/cost-core/internal/azurecost"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// Store is the pgx-backed cost ledger.
type Store struct {
	pool *pgxpool.Pool
}

// New constructs a Store from a connection pool. pool must not be nil.
func New(pool *pgxpool.Pool) (*Store, error) {
	if pool == nil {
		return nil, errors.New("postgres ledger: pool must not be nil")
	}
	return &Store{pool: pool}, nil
}

// Connect opens a pgx pool against dsn and verifies connectivity with a ping.
// The caller owns the returned pool and must Close it.
func Connect(ctx context.Context, dsn string) (*pgxpool.Pool, error) {
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		return nil, fmt.Errorf("postgres ledger: new pool: %w", err)
	}
	pingCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if err := pool.Ping(pingCtx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("postgres ledger: ping: %w", err)
	}
	return pool, nil
}

// Ensure Store satisfies the Ledger interface.
var _ ledger.Ledger = (*Store)(nil)
var _ azurecost.Repository = (*Store)(nil)
var _ azurecost.Reader = (*Store)(nil)

// ReplaceAzureActualCosts atomically replaces the queried billing window for
// one Azure scope. This clears meters Azure removed during a correction while
// preserving the previous successful snapshot if an import write fails.
func (s *Store) ReplaceAzureActualCosts(ctx context.Context, scope string, from, to time.Time, projectOrgs map[string]string, rows []azurecost.Row) error {
	if scope == "" || from.IsZero() || to.IsZero() || !from.Before(to) {
		return fmt.Errorf("postgres Azure cost import: invalid replacement window")
	}
	const query = `
		INSERT INTO azure_actual_costs
			(azure_scope, project_tag, meter, usage_date, amount, currency, org_id, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, now())
		ON CONFLICT (azure_scope, project_tag, meter, usage_date, currency)
		DO UPDATE SET amount = EXCLUDED.amount, org_id = EXCLUDED.org_id, updated_at = now()`
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("postgres Azure cost import: begin transaction: %w", err)
	}
	defer tx.Rollback(ctx)
	if _, err := tx.Exec(ctx, `DELETE FROM azure_actual_costs WHERE azure_scope = $1 AND usage_date >= $2::date AND usage_date < $3::date`, scope, from.UTC().Truncate(24*time.Hour), to.UTC().Truncate(24*time.Hour)); err != nil {
		return fmt.Errorf("postgres Azure cost import: clear replaced window: %w", err)
	}
	for _, row := range rows {
		if row.Scope != scope || row.Project == "" || row.Meter == "" || row.Date.Before(from) || !row.Date.Before(to) || row.Currency == "" {
			return fmt.Errorf("postgres Azure cost import: incomplete source row")
		}
		orgID := projectOrgs[row.Project]
		if _, err := tx.Exec(ctx, query, row.Scope, row.Project, row.Meter, row.Date.UTC().Truncate(24*time.Hour), row.Amount, row.Currency, orgID); err != nil {
			return fmt.Errorf("postgres Azure cost import: upsert daily row: %w", err)
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("postgres Azure cost import: commit transaction: %w", err)
	}
	return nil
}

// AzureActualSummary returns invoice-source costs for one organization. It is
// deliberately org-wide: Azure's project-level billing data has no verified
// Verevon user identity.
func (s *Store) AzureActualSummary(ctx context.Context, orgID string, from, to *time.Time) (azurecost.Summary, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT currency, SUM(amount)::double precision, COUNT(*), MAX(usage_date)
		FROM azure_actual_costs
		WHERE org_id = $1
		  AND ($2::date IS NULL OR usage_date >= $2::date)
		  AND ($3::date IS NULL OR usage_date <= $3::date)
		GROUP BY currency
		ORDER BY currency`, orgID, from, to)
	if err != nil {
		return azurecost.Summary{}, fmt.Errorf("postgres Azure cost summary: query: %w", err)
	}
	defer rows.Close()
	summary := azurecost.Summary{Available: true, ByCurrency: []azurecost.CurrencyTotal{}}
	for rows.Next() {
		var currency string
		var total float64
		var count int64
		var latest time.Time
		if err := rows.Scan(&currency, &total, &count, &latest); err != nil {
			return azurecost.Summary{}, fmt.Errorf("postgres Azure cost summary: scan: %w", err)
		}
		summary.ByCurrency = append(summary.ByCurrency, azurecost.CurrencyTotal{Currency: currency, Amount: total})
		summary.Rows += count
		if summary.LatestUsageDate == nil || latest.After(*summary.LatestUsageDate) {
			date := latest
			summary.LatestUsageDate = &date
		}
	}
	if err := rows.Err(); err != nil {
		return azurecost.Summary{}, fmt.Errorf("postgres Azure cost summary: rows: %w", err)
	}
	return summary, nil
}

// RecordEntry appends a cost event. A non-empty idempotency key dedupes
// retries via an ON CONFLICT DO NOTHING on the unique index.
func (s *Store) RecordEntry(ctx context.Context, e ledger.Entry) error {
	if err := ledger.ValidateEntry(e); err != nil {
		return err
	}
	id, err := uuid.NewRandom()
	if err != nil {
		return fmt.Errorf("postgres ledger: new id: %w", err)
	}
	createdAt := e.CreatedAt
	if createdAt.IsZero() {
		createdAt = time.Now().UTC()
	}

	scopedKey := scopedIdempotencyKey(e.OrgID, e.IdempotencyKey)
	// An unknown cost basis is stored as NULL (migration 0005), so a reader
	// can tell "the producer did not say" from a reported basis.
	const q = `
		INSERT INTO cost_entries
			(id, org_id, user_id, producer_id, run_id, request_id, model,
			 input_tokens, output_tokens, cost_usd, idempotency_key, created_at,
			 cache_read_input_tokens, cache_creation_input_tokens, cost_basis, provider_used)
		SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $14, $15, NULLIF($16::text, ''), $17
		WHERE $13 = '' OR NOT EXISTS (
			SELECT 1 FROM cost_entries
			WHERE org_id = $2 AND idempotency_key IN ($11, $13)
		)
		ON CONFLICT DO NOTHING`

	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("postgres ledger: begin record: %w", err)
	}
	defer tx.Rollback(ctx)
	// Serialize ingestion and erasure within this owner plane. A replay that
	// arrives concurrently with a purge cannot restore an erased ledger.
	if _, err = tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1,0))`, "cost-ledger:"+e.OrgID); err != nil {
		return err
	}
	var erased bool
	if err = tx.QueryRow(ctx, `SELECT EXISTS(SELECT 1 FROM cost_erased_organizations WHERE org_hash=$1)`, ledger.ErasedOrgHash(e.OrgID)).Scan(&erased); err != nil {
		return err
	}
	if erased {
		return ledger.ErrOrgErased
	}
	inserted, err := tx.Exec(ctx, q,
		id, e.OrgID, e.UserID, e.ProducerID, e.RunID, e.RequestID, e.Model,
		e.InputTokens, e.OutputTokens, e.CostUSD, scopedKey, createdAt, e.IdempotencyKey,
		e.CacheReadInputTokens, e.CacheCreationInputTokens, e.CostBasis, e.ProviderUsed,
	)
	if err != nil {
		return fmt.Errorf("postgres ledger: insert entry: %w", err)
	}
	if inserted.RowsAffected() == 1 && (e.CostBasis == "metered" || e.CostBasis == "subscription") {
		if _, err := tx.Exec(ctx, `INSERT INTO cost_billing_outbox(entry_id) VALUES($1) ON CONFLICT DO NOTHING`, id); err != nil {
			return fmt.Errorf("postgres ledger: enqueue billing usage: %w", err)
		}
	}
	return tx.Commit(ctx)
}

// GetUsage rolls up totals for an org+user. ErrUsageNotFound when no entries.
func (s *Store) GetUsage(ctx context.Context, orgID, userID string) (*ledger.Usage, error) {
	u, found, err := s.aggregateRow(ctx, `
		SELECT
			COALESCE(SUM(input_tokens), 0),
			COALESCE(SUM(output_tokens), 0),
			COALESCE(SUM(cost_usd), 0)::double precision,
			COUNT(*),
			COALESCE(SUM(cache_read_input_tokens), 0),
			COALESCE(SUM(cache_creation_input_tokens), 0),
			`+meteredTokenColumns+`,
			`+costBasisReportColumns+`,
			COALESCE(MAX(created_at), 'epoch'::timestamptz)
		FROM cost_entries
		WHERE org_id = $1 AND user_id = $2`, orgID, userID)
	if err != nil {
		return nil, err
	}
	if !found {
		return nil, ledger.ErrUsageNotFound
	}
	u.OrgID = orgID
	u.UserID = userID
	return u, nil
}

// GetRunUsage rolls up totals for a single run. ErrUsageNotFound when empty.
func (s *Store) GetRunUsage(ctx context.Context, runID string) (*ledger.Usage, error) {
	if runID == "" {
		return nil, ledger.ErrUsageNotFound
	}
	row := s.pool.QueryRow(ctx, `
		SELECT
			COALESCE(MAX(org_id), ''),
			COALESCE(SUM(input_tokens), 0),
			COALESCE(SUM(output_tokens), 0),
			COALESCE(SUM(cost_usd), 0)::double precision,
			COUNT(*),
			COALESCE(SUM(cache_read_input_tokens), 0),
			COALESCE(SUM(cache_creation_input_tokens), 0),
			`+costBasisReportColumns+`,
			COALESCE(MAX(created_at), 'epoch'::timestamptz)
		FROM cost_entries
		WHERE run_id = $1`, runID)

	var (
		orgID string
		u     ledger.Usage
	)
	if err := row.Scan(
		&orgID, &u.TotalInputTokens, &u.TotalOutputTokens, &u.TotalCostUSD, &u.EntryCount,
		&u.TotalCacheReadInputTokens, &u.TotalCacheCreationInputTokens,
		&u.MeteredEntryCount, &u.SubscriptionEntryCount, &u.UnknownBasisEntryCount,
		&u.MeteredCostUSD, &u.SubscriptionInputTokens, &u.SubscriptionOutputTokens,
		&u.UnknownBasisInputTokens, &u.UnknownBasisOutputTokens, &u.ZeroTokenEntryCount,
		&u.LatestEntryAt,
	); err != nil {
		return nil, fmt.Errorf("postgres ledger: run usage: %w", err)
	}
	if u.EntryCount == 0 {
		return nil, ledger.ErrUsageNotFound
	}
	u.OrgID = orgID
	u.RunID = runID
	return &u, nil
}

// Aggregate rolls up totals across all entries matching the filter. Never
// returns ErrUsageNotFound — an empty match yields a zero-valued Usage.
func (s *Store) Aggregate(ctx context.Context, f ledger.AggregateFilter) (*ledger.Usage, error) {
	where, args := buildWhere(f)
	q := `
		SELECT
			COALESCE(SUM(input_tokens), 0),
			COALESCE(SUM(output_tokens), 0),
			COALESCE(SUM(cost_usd), 0)::double precision,
			COUNT(*),
			COALESCE(SUM(cache_read_input_tokens), 0),
			COALESCE(SUM(cache_creation_input_tokens), 0),
			` + meteredTokenColumns + `,
			` + costBasisReportColumns + `,
			COALESCE(MAX(created_at), 'epoch'::timestamptz)
		FROM cost_entries` + where

	u, _, err := s.aggregateRowArgs(ctx, q, args)
	if err != nil {
		return nil, err
	}
	u.OrgID = f.OrgID
	u.UserID = f.UserID
	u.RunID = f.RunID
	return u, nil
}

// ListEntries returns the most recent matching entries, newest first.
func (s *Store) ListEntries(ctx context.Context, f ledger.AggregateFilter, limit int) ([]ledger.Entry, error) {
	if limit <= 0 {
		limit = 100
	}
	where, args := buildWhere(f)
	args = append(args, limit)
	q := `
		SELECT org_id, user_id, producer_id, run_id, request_id, model,
			input_tokens, output_tokens, cost_usd::double precision,
			idempotency_key, created_at,
			cache_read_input_tokens, cache_creation_input_tokens,
			COALESCE(cost_basis, ''), provider_used
		FROM cost_entries` + where + `
		ORDER BY created_at DESC
		LIMIT $` + itoa(len(args))

	rows, err := s.pool.Query(ctx, q, args...)
	if err != nil {
		return nil, fmt.Errorf("postgres ledger: list entries: %w", err)
	}
	defer rows.Close()

	var out []ledger.Entry
	for rows.Next() {
		var e ledger.Entry
		if err := rows.Scan(
			&e.OrgID, &e.UserID, &e.ProducerID, &e.RunID, &e.RequestID, &e.Model,
			&e.InputTokens, &e.OutputTokens, &e.CostUSD, &e.IdempotencyKey, &e.CreatedAt,
			&e.CacheReadInputTokens, &e.CacheCreationInputTokens, &e.CostBasis, &e.ProviderUsed,
		); err != nil {
			return nil, fmt.Errorf("postgres ledger: scan entry: %w", err)
		}
		out = append(out, e)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("postgres ledger: rows: %w", err)
	}
	return out, nil
}

// scopedIdempotencyKey maps a caller key into the existing globally-unique
// column without dropping the legacy index. Including orgID makes new writes
// tenant-scoped, while the INSERT's legacy-key lookup still deduplicates rows
// written by the previous binary. This is rollback-compatible and requires no
// destructive rewrite of existing accounting data.
func scopedIdempotencyKey(orgID, key string) string {
	if key == "" {
		return ""
	}
	digest := sha256.Sum256([]byte(orgID + "\x00" + key))
	return fmt.Sprintf("v2:%x", digest[:])
}

// CheckBudget verifies accumulated org+user usage against caps.
func (s *Store) CheckBudget(ctx context.Context, orgID, userID string, maxCostUSD float64, maxTokens int64) error {
	if err := ledger.ValidateScope(orgID, userID); err != nil {
		return err
	}
	if err := ledger.ValidateBudget(maxCostUSD, maxTokens); err != nil {
		return err
	}
	u, err := s.GetUsage(ctx, orgID, userID)
	if err != nil {
		if errors.Is(err, ledger.ErrUsageNotFound) {
			return nil // no usage yet — within budget
		}
		return err
	}
	if maxCostUSD > 0 && u.TotalCostUSD >= maxCostUSD {
		return fmt.Errorf("%w: current %.6f >= limit %.6f", ledger.ErrBudgetExceededCost, u.TotalCostUSD, maxCostUSD)
	}
	totalTokens := u.MeteredInputTokens + u.MeteredOutputTokens
	if maxTokens > 0 && totalTokens >= maxTokens {
		return fmt.Errorf("%w: current %d >= limit %d", ledger.ErrBudgetExceededTokens, totalTokens, maxTokens)
	}
	return nil
}

// PurgeOrg permanently deletes every cost_entries row for orgID. Called by the
// GDPR org-erasure consumer (internal/consumers/org_erasure_consumer.go) only
// for subject_type=="organization" events. A plain DELETE scoped by org_id is
// naturally idempotent (a redelivered event just deletes zero rows) and never
// touches another org's rows.
func (s *Store) PurgeOrg(ctx context.Context, orgID string) error {
	if strings.TrimSpace(orgID) == "" {
		return fmt.Errorf("postgres ledger: org_id is required")
	}
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer tx.Rollback(ctx)
	if _, err = tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1,0))`, "cost-ledger:"+orgID); err != nil {
		return err
	}
	if _, err = tx.Exec(ctx, `INSERT INTO cost_erased_organizations(org_hash) VALUES($1) ON CONFLICT DO NOTHING`, ledger.ErasedOrgHash(orgID)); err != nil {
		return err
	}
	if _, err := tx.Exec(ctx, `DELETE FROM cost_entries WHERE org_id = $1`, orgID); err != nil {
		return fmt.Errorf("postgres ledger: purge org: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM usage_reporting_periods WHERE org_id = $1`, orgID); err != nil {
		return fmt.Errorf("postgres ledger: purge reporting periods: %w", err)
	}
	return tx.Commit(ctx)
}

// meteredTokenColumns are the last two columns of every aggregateRow query:
// the input and output tokens of the entries that are not subscription
// entries, which a token budget caps. A NULL cost_basis (unreported) counts
// as metered.
const meteredTokenColumns = `COALESCE(SUM(input_tokens) FILTER (WHERE cost_basis IS DISTINCT FROM 'subscription'), 0),
			COALESCE(SUM(output_tokens) FILTER (WHERE cost_basis IS DISTINCT FROM 'subscription'), 0)`

// These reporting columns remain separate from MeteredInputTokens, whose
// conservative definition is retained for budget enforcement.
const costBasisReportColumns = `COUNT(*) FILTER (WHERE cost_basis = 'metered'),
			COUNT(*) FILTER (WHERE cost_basis = 'subscription'),
			COUNT(*) FILTER (WHERE cost_basis IS NULL),
			COALESCE(SUM(cost_usd) FILTER (WHERE cost_basis = 'metered'), 0)::double precision,
			COALESCE(SUM(input_tokens) FILTER (WHERE cost_basis = 'subscription'), 0),
			COALESCE(SUM(output_tokens) FILTER (WHERE cost_basis = 'subscription'), 0),
			COALESCE(SUM(input_tokens) FILTER (WHERE cost_basis IS NULL), 0),
			COALESCE(SUM(output_tokens) FILTER (WHERE cost_basis IS NULL), 0),
			COUNT(*) FILTER (WHERE input_tokens = 0 AND output_tokens = 0)`

// aggregateRow runs one bounded aggregate query and reports whether any row
// matched (EntryCount > 0).
func (s *Store) aggregateRow(ctx context.Context, q string, args ...any) (*ledger.Usage, bool, error) {
	return s.aggregateRowArgs(ctx, q, args)
}

func (s *Store) aggregateRowArgs(ctx context.Context, q string, args []any) (*ledger.Usage, bool, error) {
	row := s.pool.QueryRow(ctx, q, args...)
	var u ledger.Usage
	if err := row.Scan(
		&u.TotalInputTokens, &u.TotalOutputTokens, &u.TotalCostUSD, &u.EntryCount,
		&u.TotalCacheReadInputTokens, &u.TotalCacheCreationInputTokens,
		&u.MeteredInputTokens, &u.MeteredOutputTokens,
		&u.MeteredEntryCount, &u.SubscriptionEntryCount, &u.UnknownBasisEntryCount,
		&u.MeteredCostUSD, &u.SubscriptionInputTokens, &u.SubscriptionOutputTokens,
		&u.UnknownBasisInputTokens, &u.UnknownBasisOutputTokens, &u.ZeroTokenEntryCount,
		&u.LatestEntryAt,
	); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return &ledger.Usage{}, false, nil
		}
		return nil, false, fmt.Errorf("postgres ledger: aggregate: %w", err)
	}
	return &u, u.EntryCount > 0, nil
}

// buildWhere assembles a parameterised WHERE clause from a filter. Returns the
// clause (with a leading space, or empty) and the ordered argument slice.
func buildWhere(f ledger.AggregateFilter) (string, []any) {
	var (
		clauses []string
		args    []any
	)
	add := func(col string, val string) {
		if val == "" {
			return
		}
		args = append(args, val)
		clauses = append(clauses, col+" = $"+itoa(len(args)))
	}
	add("org_id", f.OrgID)
	add("user_id", f.UserID)
	add("run_id", f.RunID)
	add("model", f.Model)
	if !f.Since.IsZero() {
		args = append(args, f.Since)
		clauses = append(clauses, "created_at >= $"+itoa(len(args)))
	}
	if !f.Until.IsZero() {
		args = append(args, f.Until)
		clauses = append(clauses, "created_at <= $"+itoa(len(args)))
	}
	if len(clauses) == 0 {
		return "", args
	}
	return " WHERE " + strings.Join(clauses, " AND "), args
}

// itoa formats a small positive int without importing strconv at every call
// site; placeholder indices are always >= 1.
func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	var buf [20]byte
	i := len(buf)
	for n > 0 {
		i--
		buf[i] = byte('0' + n%10)
		n /= 10
	}
	return string(buf[i:])
}

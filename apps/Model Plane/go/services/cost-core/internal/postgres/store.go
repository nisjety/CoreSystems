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
	const q = `
		INSERT INTO cost_entries
			(id, org_id, user_id, producer_id, run_id, request_id, model,
			 input_tokens, output_tokens, cost_usd, idempotency_key, created_at)
		SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12
		WHERE $13 = '' OR NOT EXISTS (
			SELECT 1 FROM cost_entries
			WHERE org_id = $2 AND idempotency_key IN ($11, $13)
		)
		ON CONFLICT DO NOTHING`

	_, err = s.pool.Exec(ctx, q,
		id, e.OrgID, e.UserID, e.ProducerID, e.RunID, e.RequestID, e.Model,
		e.InputTokens, e.OutputTokens, e.CostUSD, scopedKey, createdAt, e.IdempotencyKey,
	)
	if err != nil {
		return fmt.Errorf("postgres ledger: insert entry: %w", err)
	}
	return nil
}

// GetUsage rolls up totals for an org+user. ErrUsageNotFound when no entries.
func (s *Store) GetUsage(ctx context.Context, orgID, userID string) (*ledger.Usage, error) {
	u, found, err := s.aggregateRow(ctx, `
		SELECT
			COALESCE(SUM(input_tokens), 0),
			COALESCE(SUM(output_tokens), 0),
			COALESCE(SUM(cost_usd), 0)::double precision,
			COUNT(*)
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
			COUNT(*)
		FROM cost_entries
		WHERE run_id = $1`, runID)

	var (
		orgID string
		u     ledger.Usage
	)
	if err := row.Scan(&orgID, &u.TotalInputTokens, &u.TotalOutputTokens, &u.TotalCostUSD, &u.EntryCount); err != nil {
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
			COUNT(*)
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
			idempotency_key, created_at
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
	totalTokens := u.TotalInputTokens + u.TotalOutputTokens
	if maxTokens > 0 && totalTokens >= maxTokens {
		return fmt.Errorf("%w: current %d >= limit %d", ledger.ErrBudgetExceededTokens, totalTokens, maxTokens)
	}
	return nil
}

// aggregateRow runs a fixed 4-column aggregate query and reports whether any
// row matched (EntryCount > 0).
func (s *Store) aggregateRow(ctx context.Context, q string, args ...any) (*ledger.Usage, bool, error) {
	return s.aggregateRowArgs(ctx, q, args)
}

func (s *Store) aggregateRowArgs(ctx context.Context, q string, args []any) (*ledger.Usage, bool, error) {
	row := s.pool.QueryRow(ctx, q, args...)
	var u ledger.Usage
	if err := row.Scan(&u.TotalInputTokens, &u.TotalOutputTokens, &u.TotalCostUSD, &u.EntryCount); err != nil {
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

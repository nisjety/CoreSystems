// Package cost queries the cost_events ledger and produces consumption
// summaries per org. The events are written by data-orchestrator-go from
// NATS.
package cost

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/shared/go/orgscope"
)

type Query struct {
	pool *pgxpool.Pool
}

func NewQuery(pool *pgxpool.Pool) *Query {
	return &Query{pool: pool}
}

type Bucket struct {
	EventType   string `json:"event_type"`
	Model       string `json:"model"`
	EventCount  int    `json:"event_count"`
	UnitsTotal  int    `json:"units_total"`  // sum(count) — chunks/queries processed
	TokensTotal int64  `json:"tokens_total"` // sum(estimated_tokens)
}

type Summary struct {
	OrgID   string    `json:"org_id"`
	From    time.Time `json:"from"`
	To      time.Time `json:"to"`
	Buckets []Bucket  `json:"buckets"`
	// Aggregate totals across all buckets
	EventCount  int   `json:"event_count"`
	UnitsTotal  int   `json:"units_total"`
	TokensTotal int64 `json:"tokens_total"`
}

// Summary returns aggregated cost data for an org over [from, to].
// Pass zero From to default to 30 days ago; zero To defaults to now.
//
// Phase 1 RLS: orgID comes from verified caller claims (authctx) via the
// /v1/cost/summary handler, so this serves exactly one tenant. Billing-shaped
// output makes the policy backstop worth having — a dropped org_id filter here
// would leak another tenant's consumption volumes, not just row counts.
func (q *Query) Summary(ctx context.Context, orgID string, from, to time.Time) (*Summary, error) {
	if to.IsZero() {
		to = time.Now().UTC()
	}
	if from.IsZero() {
		from = to.Add(-30 * 24 * time.Hour)
	}

	return orgscope.InOrgScope(ctx, q.pool, orgID,
		func(ctx context.Context, tx pgx.Tx) (*Summary, error) {
			rows, err := tx.Query(ctx, `
				SELECT event_type, model,
				       COUNT(*)               AS event_count,
				       COALESCE(SUM(count), 0)            AS units_total,
				       COALESCE(SUM(estimated_tokens), 0) AS tokens_total
				FROM cost_events
				WHERE org_id = $1
				  AND created_at >= $2
				  AND created_at <  $3
				GROUP BY event_type, model
				ORDER BY tokens_total DESC
			`, orgID, from, to)
			if err != nil {
				return nil, fmt.Errorf("query cost events: %w", err)
			}
			defer rows.Close()

			// Drained inside the scope: rows die at COMMIT.
			summary := &Summary{OrgID: orgID, From: from, To: to, Buckets: []Bucket{}}
			for rows.Next() {
				var b Bucket
				if err := rows.Scan(&b.EventType, &b.Model, &b.EventCount, &b.UnitsTotal, &b.TokensTotal); err != nil {
					return nil, fmt.Errorf("scan bucket: %w", err)
				}
				summary.Buckets = append(summary.Buckets, b)
				summary.EventCount += b.EventCount
				summary.UnitsTotal += b.UnitsTotal
				summary.TokensTotal += b.TokensTotal
			}
			if err := rows.Err(); err != nil {
				return nil, err
			}
			return summary, nil
		})
}

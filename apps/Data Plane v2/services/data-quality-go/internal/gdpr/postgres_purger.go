package gdpr

import (
	"context"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5/pgxpool"
)

// PostgresOrgPurger hard-deletes every row this service owns for one org:
// quality_eval_runs, eval_golden_judgments and org_processing_policy. All are
// org_id-scoped per infra/postgres/migrations/
// 20260711160000_quality_orchestrator_durability.sql (quality_eval_runs.org_id
// TEXT NOT NULL), 20260717100000_eval_golden_judgments.sql
// (eval_golden_judgments' PRIMARY KEY (org_id, query_norm)) and
// 20261003130000_context_provenance.sql (org_processing_policy's PRIMARY KEY
// org_id: an operator's policy row, with its reason and the operator's id, is
// org data and does not survive the org's erasure). It deliberately
// does NOT touch data_orchestrator_jobs — that table lives in this same
// Postgres instance but is owned by the sibling data-orchestrator-go
// service's own repository code, not this one — nor admin_audit_log, which the
// documents-api org purge already erases for the org.
type PostgresOrgPurger struct {
	pool *pgxpool.Pool
}

// purgeStatements are the statements HardPurgeByOrg runs, in order. The purger
// lists its tables explicitly (an unlisted table survives an erasure), each is a
// static `DELETE ... WHERE org_id = $1`, and a test pins both facts.
var purgeStatements = [...]struct{ table, sql string }{
	{"quality_eval_runs", `DELETE FROM quality_eval_runs WHERE org_id = $1`},
	{"eval_golden_judgments", `DELETE FROM eval_golden_judgments WHERE org_id = $1`},
	{"org_processing_policy", `DELETE FROM org_processing_policy WHERE org_id = $1`},
}

func NewPostgresOrgPurger(pool *pgxpool.Pool) *PostgresOrgPurger {
	return &PostgresOrgPurger{pool: pool}
}

// HardPurgeByOrg runs in a single transaction so a crash mid-purge never
// leaves the org half-purged.
//
// Safety: every statement in purgeStatements is a static
// `DELETE ... WHERE org_id = $1` literal — there is no code path here that
// could ever filter on anything else, so a caller cannot make this touch
// another org's rows even by accident. Callers (org_purge.go's HandleOrgErasure) must still pass the
// org_id straight from the event payload, never a derived or client-supplied
// value.
//
// Idempotency: this is the cross-plane erasure fan-out contract's consumer
// side, and NATS is at-least-once delivery. Every statement here is a plain
// DELETE with no compensating INSERT (no audit-log row, no tombstone), so a
// redelivery after the first successful purge simply matches zero rows on
// every table and returns nil — nothing here needs a dedup key to stay safe
// on a second run.
//
// Phase 1 RLS: deliberately NOT wrapped in orgscope.WithOrgScope, unlike every
// request-scoped path in this service. This matches wiki-store-go's identical
// decision in internal/repo/org_purge.go, and the reasoning holds even though
// this purge targets a single org:
//
//   - Under RLS a DELETE can only remove rows the policy lets the role see.
//     Any row whose org_id drifted — legacy, NULL, mis-backfilled — would
//     survive the purge silently while HardPurgeByOrg still returned nil. An
//     erasure that under-deletes and reports success is a worse failure than
//     one that runs unfiltered, because nothing downstream would notice and
//     the GDPR obligation would be quietly unmet.
//   - The isolation a policy would add is already present statically: both
//     statements below are literal `DELETE ... WHERE org_id = $1` with no code
//     path that can filter on anything else (see the Safety note above). There
//     is no dynamic filter here for a policy to backstop.
//
// The org_id arrives straight from the erasure event payload (org_purge.go's
// HandleOrgErasure), so the single bound parameter is the whole tenant
// boundary — audited, not assumed.
func (p *PostgresOrgPurger) HardPurgeByOrg(ctx context.Context, orgID string) error {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return fmt.Errorf("hard purge requires a non-empty org_id")
	}

	tx, err := p.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin org purge: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck -- commit owns the successful path.

	for _, statement := range purgeStatements {
		if _, err := tx.Exec(ctx, statement.sql, orgID); err != nil {
			return fmt.Errorf("purge %s: %w", statement.table, err)
		}
	}

	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit org purge: %w", err)
	}
	return nil
}

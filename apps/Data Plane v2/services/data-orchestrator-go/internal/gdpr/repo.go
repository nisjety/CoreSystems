package gdpr

import (
	"context"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5/pgxpool"
)

// PurgeRepo implements OrgPurger against this service's own tables.
type PurgeRepo struct {
	pool *pgxpool.Pool
}

func NewPurgeRepo(pool *pgxpool.Pool) *PurgeRepo {
	return &PurgeRepo{pool: pool}
}

// HardPurgeByOrg irrecoverably deletes every row this service owns for
// orgID: data_orchestrator_jobs and cost_events. Runs in a single
// transaction so a crash mid-purge never leaves the org half-purged.
//
// Safety: every statement below is a static `DELETE ... WHERE org_id = $1`
// literal — there is no code path here that could ever filter on anything
// else, so a caller cannot make this touch another org's rows even by
// accident. Callers (org_purge.go's HandleOrgErasure) must still pass the
// org_id straight from the event payload, never a derived or
// client-supplied value.
//
// Idempotency: this is the cross-plane erasure fan-out contract's consumer
// side (see org_purge.go), and NATS is at-least-once delivery. Every
// statement here is a plain DELETE with no compensating INSERT (no audit-log
// row, no tombstone), so a redelivery after the first successful purge
// simply matches zero rows on every table and returns nil — nothing here
// needs a dedup key to stay safe on a second run.
//
// Scope note: this only covers tables this service's own runtime code
// writes (confirmed by auditing internal/jobs and internal/cost against
// apps/Data Plane v2/infra/postgres/migrations — see org_purge.go's package
// doc for the full accounting, including tables deliberately NOT purged
// here).
func (r *PurgeRepo) HardPurgeByOrg(ctx context.Context, orgID string) error {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return fmt.Errorf("hard purge requires a non-empty org_id")
	}

	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin org purge: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck -- commit below owns the successful path.

	if _, err := tx.Exec(ctx, `DELETE FROM data_orchestrator_jobs WHERE org_id = $1`, orgID); err != nil {
		return fmt.Errorf("purge data_orchestrator_jobs: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM cost_events WHERE org_id = $1`, orgID); err != nil {
		return fmt.Errorf("purge cost_events: %w", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit org purge: %w", err)
	}
	return nil
}

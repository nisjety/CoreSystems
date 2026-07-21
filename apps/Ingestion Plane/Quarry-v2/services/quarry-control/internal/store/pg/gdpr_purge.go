// GDPR org-erasure hard purge — Postgres counterpart to the in-memory
// store's PurgeOrg (internal/store/gdpr_purge.go). See that file's package
// doc for the full enumeration of which tables actually carry an org_id
// column and why the rest (stores, snapshots, artifacts, profiles,
// webhooks, webhook_deliveries, blocklist_entries, events) are deliberately
// out of scope.
//
// Every statement below is a direct `DELETE ... WHERE org_id = $1` — no
// joins, no derived subject sets — per the safety rule that a purge query
// must never be able to reach another tenant's rows. All five run inside
// one transaction so a mid-purge failure never leaves a partial erasure
// committed; each individual statement is also independently idempotent (a
// repeat run matches zero rows), so retrying the whole call after a
// transaction-level failure is safe too — this is the "plain DELETE ...
// WHERE org_id = $1 style statements are idempotent by nature" case, with
// no non-idempotent side effect (no per-delivery audit-log append, no
// counter) added on top.
package pg

import (
	"context"
	"fmt"
	"strings"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// PurgeOrg hard-deletes every row for orgID across jobs, schedules,
// quarry_sources, quarry_benchmarks, and quarry_idempotency_keys.
func (d *postgresDB) PurgeOrg(orgID string) (store.PurgeResult, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return store.PurgeResult{}, fmt.Errorf("pg: PurgeOrg requires a non-empty orgID")
	}

	ctx := context.Background()
	tx, err := d.pool.Begin(ctx)
	if err != nil {
		return store.PurgeResult{}, fmt.Errorf("pg: PurgeOrg begin tx: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }() // no-op once Commit succeeds

	del := func(query string) (int64, error) {
		tag, err := tx.Exec(ctx, query, orgID)
		if err != nil {
			return 0, err
		}
		return tag.RowsAffected(), nil
	}

	var res store.PurgeResult
	var derr error
	if res.JobsDeleted, derr = del(`DELETE FROM jobs WHERE org_id = $1`); derr != nil {
		return store.PurgeResult{}, fmt.Errorf("pg: PurgeOrg jobs: %w", derr)
	}
	if res.SchedulesDeleted, derr = del(`DELETE FROM schedules WHERE org_id = $1`); derr != nil {
		return store.PurgeResult{}, fmt.Errorf("pg: PurgeOrg schedules: %w", derr)
	}
	if res.SourcesDeleted, derr = del(`DELETE FROM quarry_sources WHERE org_id = $1`); derr != nil {
		return store.PurgeResult{}, fmt.Errorf("pg: PurgeOrg quarry_sources: %w", derr)
	}
	if res.BenchmarksDeleted, derr = del(`DELETE FROM quarry_benchmarks WHERE org_id = $1`); derr != nil {
		return store.PurgeResult{}, fmt.Errorf("pg: PurgeOrg quarry_benchmarks: %w", derr)
	}
	if res.IdempotencyKeysDeleted, derr = del(`DELETE FROM quarry_idempotency_keys WHERE org_id = $1`); derr != nil {
		return store.PurgeResult{}, fmt.Errorf("pg: PurgeOrg quarry_idempotency_keys: %w", derr)
	}

	if err := tx.Commit(ctx); err != nil {
		return store.PurgeResult{}, fmt.Errorf("pg: PurgeOrg commit: %w", err)
	}
	return res, nil
}

// GDPR org-erasure hard purge — the Data-Plane wiki-store-go half of
// ORGANIZATION hard-erasure. The counterpart consumer that decodes the
// cross-plane fan-out event and calls this method lives in
// internal/gdpr/org_purge.go; see that file's package doc for the full
// producer/consumer contract (org-core publishes
// "verevon.gdpr.erasure.requested" from both its explicit hard-delete path
// and its 30-day retention cron).
//
// Scope: HardPurgeByOrg hard-deletes (not soft-deletes) every row this
// service owns for the org:
//
//   - wiki_pages                          — the wiki tree root for the org.
//   - wiki_page_versions, wiki_proposals  — no org_id column of their own;
//     carry an ON DELETE CASCADE FK to wiki_pages(page_id), so deleting the
//     org's rows from wiki_pages purges these transitively. Not a separate
//     statement — verified against the current schema
//     (infra/postgres/init.sql), not assumed.
//   - wiki_source_logs, wiki_maintenance_logs — carry their own NOT NULL
//     org_id column (backfilled by the 2026-07-10 reconciliation migration)
//     in addition to a page_id FK to wiki_pages. Purged explicitly by
//     org_id, ahead of wiki_pages, so this purge never depends on cascade
//     ordering to be complete.
//   - operating_maps                      — one row per org (org_id UNIQUE).
//   - operating_map_versions, operating_map_proposals,
//     operating_map_blueprint_suggestions — no independent purge statement:
//     each carries an ON DELETE CASCADE FK to operating_maps
//     (operating_map_id), so deleting the org's operating_maps row purges
//     the entire Operating Map tree for that org.
//   - wiki_event_outbox                   — its own org_id column, no FK to
//     wiki_pages; purged explicitly.
//
// Does NOT purge documents, chunks, embeddings, graph data, or retrieval
// traces for the org: those tables live in sibling Data Plane v2 services
// (documents-api-go, embedding-engine-rs, index-engine-rs, graph-index-rs,
// retrieval-engine-rs, data-quality-go, data-orchestrator-go,
// quickwit-adapter-rs) with their own database access code this service
// cannot reach. Each of those has (or needs) its own consumer on the same
// erasure subject.
package repo

import (
	"context"
	"fmt"
	"strings"
)

// PurgeResult reports how many rows HardPurgeByOrg hard-deleted from each
// org-scoped table it directly targets. Every field is a plain row count —
// safe to log, safe to sum via Total, and (because every underlying
// statement is a `DELETE ... WHERE org_id = $1` with no compensating
// insert) safe to compare across a redelivered or duplicate call: a second
// HardPurgeByOrg for the same org returns an all-zero PurgeResult, not an
// error.
type PurgeResult struct {
	WikiPagesDeleted           int64 `json:"wiki_pages_deleted"`
	WikiSourceLogsDeleted      int64 `json:"wiki_source_logs_deleted"`
	WikiMaintenanceLogsDeleted int64 `json:"wiki_maintenance_logs_deleted"`
	OperatingMapsDeleted       int64 `json:"operating_maps_deleted"`
	WikiEventOutboxDeleted     int64 `json:"wiki_event_outbox_deleted"`
}

// Total sums every deleted-row count directly targeted above. It does NOT
// include rows removed transitively by cascade (wiki_page_versions,
// wiki_proposals, operating_map_versions, operating_map_proposals,
// operating_map_blueprint_suggestions) — Postgres doesn't report cascaded
// row counts through RowsAffected() on the parent statement, so those are
// not counted here. Convenience for logging and tests.
func (r PurgeResult) Total() int64 {
	return r.WikiPagesDeleted + r.WikiSourceLogsDeleted + r.WikiMaintenanceLogsDeleted +
		r.OperatingMapsDeleted + r.WikiEventOutboxDeleted
}

// HardPurgeByOrg irrecoverably deletes every row this service owns for
// orgID. Runs in a single transaction so a crash mid-purge never leaves the
// org half-purged.
//
// Safety: every statement below is a static `DELETE ... WHERE org_id = $1`
// literal — there is no code path here that could ever filter on anything
// else, so a caller cannot make this touch another org's rows even by
// accident. Callers (internal/gdpr/org_purge.go) must still pass the org_id
// straight from the event payload, never a derived or client-supplied
// value.
//
// Idempotency: this is the cross-plane erasure fan-out contract's consumer
// side, and NATS is at-least-once delivery. Every statement here is a plain
// DELETE with no compensating INSERT (no audit-log row, no tombstone), so a
// redelivery after the first successful purge simply matches zero rows on
// every table and returns a zero-value PurgeResult, nil error.
func (r *WikiRepo) HardPurgeByOrg(ctx context.Context, orgID string) (PurgeResult, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return PurgeResult{}, fmt.Errorf("hard purge requires a non-empty org_id")
	}

	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return PurgeResult{}, fmt.Errorf("begin wiki org purge: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }() // no-op once Commit succeeds

	del := func(query string) (int64, error) {
		tag, err := tx.Exec(ctx, query, orgID)
		if err != nil {
			return 0, err
		}
		return tag.RowsAffected(), nil
	}

	var result PurgeResult
	var derr error

	// wiki_event_outbox carries its own org_id column and no FK relationship
	// to wiki_pages — purge it explicitly, independent of the wiki tree
	// delete below.
	if result.WikiEventOutboxDeleted, derr = del(`DELETE FROM wiki_event_outbox WHERE org_id = $1`); derr != nil {
		return PurgeResult{}, fmt.Errorf("purge wiki_event_outbox: %w", derr)
	}

	// wiki_maintenance_logs / wiki_source_logs both carry their own NOT NULL
	// org_id column as well as an ON DELETE CASCADE FK to wiki_pages
	// (page_id). Delete explicitly, scoped by org_id, ahead of wiki_pages
	// below so this purge never depends on cascade ordering to be complete.
	if result.WikiMaintenanceLogsDeleted, derr = del(`DELETE FROM wiki_maintenance_logs WHERE org_id = $1`); derr != nil {
		return PurgeResult{}, fmt.Errorf("purge wiki_maintenance_logs: %w", derr)
	}
	if result.WikiSourceLogsDeleted, derr = del(`DELETE FROM wiki_source_logs WHERE org_id = $1`); derr != nil {
		return PurgeResult{}, fmt.Errorf("purge wiki_source_logs: %w", derr)
	}

	// wiki_pages is the root of the wiki tree for this org. wiki_page_versions
	// and wiki_proposals have no org_id column of their own — only a page_id
	// FK to wiki_pages(page_id) ON DELETE CASCADE — so this single statement
	// purges them transitively; there is no separate statement for either.
	if result.WikiPagesDeleted, derr = del(`DELETE FROM wiki_pages WHERE org_id = $1`); derr != nil {
		return PurgeResult{}, fmt.Errorf("purge wiki_pages: %w", derr)
	}

	// operating_maps is unique per org_id (UNIQUE constraint). Every other
	// operating_map_* table carries an ON DELETE CASCADE FK back to
	// operating_maps(operating_map_id), so this single statement purges the
	// entire Operating Map tree (versions, proposals, blueprint suggestions)
	// for the org — there is no separate statement for any of them.
	if result.OperatingMapsDeleted, derr = del(`DELETE FROM operating_maps WHERE org_id = $1`); derr != nil {
		return PurgeResult{}, fmt.Errorf("purge operating_maps: %w", derr)
	}

	if err := tx.Commit(ctx); err != nil {
		return PurgeResult{}, fmt.Errorf("commit wiki org purge: %w", err)
	}
	return result, nil
}

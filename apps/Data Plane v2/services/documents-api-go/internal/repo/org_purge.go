package repo

import (
	"context"
	"fmt"
	"strings"
)

// HardPurgeByOrg irrecoverably deletes every row this service owns for
// orgID: documents, the documents outbox, source objects, and the per-org
// cache-version counter (org_versions). Runs in a single transaction so a
// crash mid-purge never leaves the org half-purged.
//
// Safety: every statement below is a static `DELETE ... WHERE org_id = $1`
// literal — there is no code path here that could ever filter on anything
// else, so a caller cannot make this touch another org's rows even by
// accident. Callers (internal/gdpr/org_purge.go) must still pass the org_id
// straight from the event payload, never a derived or client-supplied value.
//
// Idempotency: this is the cross-plane erasure fan-out contract's consumer
// side (see internal/gdpr/org_purge.go), and NATS is at-least-once delivery.
// Every statement here is a plain DELETE with no compensating INSERT (no
// audit-log row, no tombstone), so a redelivery after the first successful
// purge simply matches zero rows on every table and returns nil — nothing
// here needs a dedup key to stay safe on a second run.
//
// Scope note: this only covers tables documents-api-go's own repository code
// queries (confirmed by auditing this package plus internal/events/outbox.go
// against apps/Data Plane v2/infra/postgres/migrations). Chunks, embeddings,
// graph data, wiki data, and retrieval traces for the same org live in
// sibling Data Plane v2 services — embedding-engine-rs, index-engine-rs,
// graph-index-rs, wiki-store-go, retrieval-engine-rs, data-quality-go,
// data-orchestrator-go, and quickwit-adapter-rs each own their own tables in
// the shared Postgres instance and are not reachable from here. See
// internal/gdpr/org_purge.go's package doc for the full list and the
// follow-up this implies.
//
// Phase 1 RLS: this path deliberately stays on the UNSCOPED (superuser) pool
// while the rest of the repository runs inside orgscope. Under row-level
// security a DELETE can only remove rows the role is permitted to SEE, so a row
// whose org_id had drifted — or gone NULL — would silently survive the erasure
// while this function still returned nil and the caller still reported success.
// An erasure that under-deletes and reports OK is strictly worse than one that
// runs unfiltered. Tenant scoping here comes from the static
// `WHERE org_id = $1` on every statement below, which no code path can widen.
// Every service in this rollout made the same call for its erasure path.
func (r *DocumentRepo) HardPurgeByOrg(ctx context.Context, orgID string) error {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return fmt.Errorf("hard purge requires a non-empty org_id")
	}

	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin org purge: %w", err)
	}
	defer tx.Rollback(ctx)

	if _, err := tx.Exec(ctx, `DELETE FROM documents_outbox WHERE org_id = $1`, orgID); err != nil {
		return fmt.Errorf("purge documents_outbox: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM documents WHERE org_id = $1`, orgID); err != nil {
		return fmt.Errorf("purge documents: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM source_objects WHERE org_id = $1`, orgID); err != nil {
		return fmt.Errorf("purge source_objects: %w", err)
	}
	if _, err := tx.Exec(ctx, `DELETE FROM org_versions WHERE org_id = $1`, orgID); err != nil {
		return fmt.Errorf("purge org_versions: %w", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit org purge: %w", err)
	}
	return nil
}

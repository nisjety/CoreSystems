package api

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/store/storetest"
)

// The live backfill, rehearsed against a real database: a 0012-shaped integration database with
// the live connection mix (2 active, 2 pending, 1 needs_refresh, 2 deleted), migration 0013, then
// the reconcile the service runs at start. Needs INTEGRATION_TEST_DATABASE_URL and fails, not
// skips, under REQUIRE_CONTRACT or CI.

func countRows(t *testing.T, pool *pgxpool.Pool, query string, args ...any) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(context.Background(), query, args...).Scan(&n); err != nil {
		t.Fatalf("%s: %v", query, err)
	}
	return n
}

func TestReconcileBackfillsLiveShapedDataOnDisposablePostgres(t *testing.T) {
	ctx := context.Background()
	pool := storetest.IsolatedPostgres(t, true)
	repo := store.NewPostgresRepository(pool)
	seedLiveShapedConnections(t, repo)
	cfg := ServerConfig{Repo: repo}

	first, err := ReconcileConnectionZDR(ctx, cfg)
	if err != nil {
		t.Fatalf("first pass: %v", err)
	}
	if first != (ZDRReconcileResult{Examined: 5, Created: 5}) {
		t.Fatalf("first pass = %+v, want 5 created", first)
	}

	// The live proof query: one row per live subscription connection, all not_supported.
	if n := countRows(t, pool, `SELECT count(*) FROM integration_connection_zdr WHERE capability = 'not_supported' AND basis = 'provider_terms_known' AND digest = $1 AND catalog_version = 1`, subscriptionDigestCatalog1); n != 5 {
		t.Fatalf("%d not_supported rows with the catalog-1 digest, want 5", n)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM integration_connection_zdr`); n != 5 {
		t.Fatalf("%d rows in total, want 5 (deleted and non-model connections are not assessed)", n)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM integration_connection_zdr z JOIN integration_connections c ON c.id = z.connection_id WHERE c.deleted_at IS NOT NULL OR c.provider_key <> $1`, codexsubscription.ProviderKey); n != 0 {
		t.Fatalf("%d records belong to a deleted or non-model connection", n)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM integration_connection_zdr_acknowledgements`); n != 0 {
		t.Fatalf("the backfill wrote %d acknowledgements", n)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM integration_audit_events WHERE event_type = 'subscription_connection.zdr_assessed' AND published_at IS NULL`); n != 5 {
		t.Fatalf("%d pending zdr_assessed audit rows, want 5 (one per assessed connection, in the same transaction)", n)
	}

	// Idempotent: a second and third pass change and audit nothing.
	var checkedAt []time.Time
	rows, err := pool.Query(ctx, `SELECT checked_at FROM integration_connection_zdr ORDER BY connection_id`)
	if err != nil {
		t.Fatal(err)
	}
	for rows.Next() {
		var at time.Time
		if err := rows.Scan(&at); err != nil {
			t.Fatal(err)
		}
		checkedAt = append(checkedAt, at)
	}
	rows.Close()
	for pass := 2; pass <= 3; pass++ {
		result, err := ReconcileConnectionZDR(ctx, cfg)
		if err != nil || result != (ZDRReconcileResult{Examined: 5, Unchanged: 5}) {
			t.Fatalf("pass %d = (%+v, %v), want 5 unchanged", pass, result, err)
		}
	}
	if n := countRows(t, pool, `SELECT count(*) FROM integration_audit_events WHERE event_type = 'subscription_connection.zdr_assessed'`); n != 5 {
		t.Fatalf("%d zdr_assessed rows after re-running, want still 5", n)
	}
	var after []time.Time
	rows, err = pool.Query(ctx, `SELECT checked_at FROM integration_connection_zdr ORDER BY connection_id`)
	if err != nil {
		t.Fatal(err)
	}
	for rows.Next() {
		var at time.Time
		if err := rows.Scan(&at); err != nil {
			t.Fatal(err)
		}
		after = append(after, at)
	}
	rows.Close()
	if len(after) != len(checkedAt) {
		t.Fatalf("row count moved: %d -> %d", len(checkedAt), len(after))
	}
	for i := range after {
		if !after[i].Equal(checkedAt[i]) {
			t.Fatalf("a re-run rewrote checked_at of row %d", i)
		}
	}
}

func TestReconcileRevokesAcknowledgementsOfAChangedDigestInOneTransactionOnDisposablePostgres(t *testing.T) {
	ctx := context.Background()
	pool := storetest.IsolatedPostgres(t, true)
	repo := store.NewPostgresRepository(pool)
	live, _, _ := seedLiveShapedConnections(t, repo)
	target := live[0]

	// A record from an older catalog, acknowledged by its owner.
	if _, err := pool.Exec(ctx, `
		INSERT INTO integration_connection_zdr (connection_id, provider_key, auth_mode, capability, basis, basis_ref, catalog_version, digest, checked_at)
		VALUES ($1, 'openai-codex-subscription', 'codex_chatgpt_device_code', 'cannot_verify', 'not_assessed', 'fallback@1', 1, $2, now())`,
		target, strings.Repeat("9", 64)); err != nil {
		t.Fatalf("seed old record: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO integration_connection_zdr_acknowledgements
			(id, organization_id, connection_id, user_id, provider_key, capability, basis, catalog_version, digest,
			 text_version, locale, text_sha256, text_snapshot, org_zdr_at_ack, acknowledged_via)
		VALUES ('ack-old', 'org-a', $1, 'user-1', 'openai-codex-subscription', 'cannot_verify', 'not_assessed', 1, $2,
			 'v1', 'no', $3, 'text', true, 'connect')`, target, strings.Repeat("9", 64), strings.Repeat("c", 64)); err != nil {
		t.Fatalf("seed old acknowledgement: %v", err)
	}

	result, err := ReconcileConnectionZDR(ctx, ServerConfig{Repo: repo})
	if err != nil {
		t.Fatalf("reconcile: %v", err)
	}
	if result != (ZDRReconcileResult{Examined: 5, Created: 4, Changed: 1, Revoked: 1}) {
		t.Fatalf("result = %+v", result)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM integration_connection_zdr_acknowledgements WHERE id = 'ack-old' AND revoked_at IS NOT NULL AND revoke_reason = 'capability_changed'`); n != 1 {
		t.Fatalf("the old acknowledgement was not revoked with reason capability_changed")
	}
	if n := countRows(t, pool, `SELECT count(*) FROM integration_audit_events WHERE connection_id = $1 AND event_type IN ('subscription_connection.zdr_assessed', 'subscription_connection.zdr_acknowledgement_invalidated')`, target); n != 2 {
		t.Fatalf("%d audit rows for the changed connection, want 2 (assessed + invalidated)", n)
	}
}

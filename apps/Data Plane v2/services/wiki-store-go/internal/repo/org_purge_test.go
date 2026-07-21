package repo

// Integration test for WikiRepo.HardPurgeByOrg — the safety contract for the
// GDPR org-erasure consumer (internal/gdpr/org_purge.go): purging org A must
// never touch org B's rows created in the same run (including rows purged
// only transitively, via ON DELETE CASCADE), and redelivering the same purge
// (NATS is at-least-once) must not error the second time.
//
// Mirrors the harness schema_integration_test.go and outbox_integration_test.go
// already use in this package: gated on WIKI_TEST_DATABASE_URL, an isolated
// schema per run, legacyWikiSchema + the wiki reconciliation migration for the
// wiki_* tables, plus the operating-maps and wiki-event-outbox migrations
// this test additionally needs.

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

const operatingMapsMigration = "20260618120000_add_operating_maps.sql"

// triggerSetUpdatedAtFn is the shared helper function
// infra/postgres/init.sql defines once (outside any single service's
// migrations) that the operating-maps migration's triggers depend on. The
// isolated test schema below never runs init.sql, so it is defined here —
// verbatim — the same way legacyWikiSchema stands in for init.sql's wiki
// tables.
const triggerSetUpdatedAtFn = `
CREATE OR REPLACE FUNCTION trigger_set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
`

// purgeScopedTables lists every table HardPurgeByOrg deletes from directly —
// kept in one place so the "org A fully purged" / "org B untouched"
// assertions below can loop over the exact same list HardPurgeByOrg's
// implementation uses.
var purgeScopedTables = []string{
	"wiki_pages", "wiki_source_logs", "wiki_maintenance_logs",
	"wiki_event_outbox", "operating_maps",
}

// cascadePurgedTables lists every table HardPurgeByOrg never targets
// directly but which must still end up empty for a purged org because of an
// ON DELETE CASCADE FK to a table that IS targeted directly (wiki_pages or
// operating_maps). None of these carry a queryable org_id column on their
// own (see org_purge.go's package doc), so counting them requires a join
// back to the parent table's org_id — countCascadeRowsForOrg below does
// that per-table.
var cascadePurgedTables = []string{
	"wiki_page_versions", "wiki_proposals",
	"operating_map_versions", "operating_map_proposals", "operating_map_blueprint_suggestions",
}

func setupPurgeTestPool(t *testing.T) *pgxpool.Pool {
	t.Helper()
	databaseURL := os.Getenv("WIKI_TEST_DATABASE_URL")
	if databaseURL == "" {
		t.Skip("set WIKI_TEST_DATABASE_URL to a disposable local *_test database")
	}
	adminConfig, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse WIKI_TEST_DATABASE_URL: %v", err)
	}
	host := adminConfig.ConnConfig.Host
	if host != "localhost" && host != "127.0.0.1" && host != "::1" {
		t.Fatalf("refusing non-local test database host %q", host)
	}
	if !strings.HasSuffix(adminConfig.ConnConfig.Database, "_test") {
		t.Fatalf("refusing database %q: name must end in _test", adminConfig.ConnConfig.Database)
	}

	ctx := context.Background()
	adminPool, err := pgxpool.NewWithConfig(ctx, adminConfig)
	if err != nil {
		t.Fatalf("connect admin pool: %v", err)
	}
	t.Cleanup(adminPool.Close)

	schema := "wiki_org_purge_" + strings.ReplaceAll(uuid.NewString(), "-", "")
	if _, err := adminPool.Exec(ctx, "CREATE SCHEMA "+schema); err != nil {
		t.Fatalf("create isolated schema: %v", err)
	}
	t.Cleanup(func() { _, _ = adminPool.Exec(context.Background(), "DROP SCHEMA "+schema+" CASCADE") })

	testConfig, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse isolated pool config: %v", err)
	}
	testConfig.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, testConfig)
	if err != nil {
		t.Fatalf("connect isolated pool: %v", err)
	}
	t.Cleanup(pool.Close)

	if _, err := pool.Exec(ctx, legacyWikiSchema); err != nil {
		t.Fatalf("create legacy wiki schema: %v", err)
	}
	if _, err := pool.Exec(ctx, readWikiSchemaMigration(t)); err != nil {
		t.Fatalf("apply wiki reconciliation migration: %v", err)
	}
	outboxMigrationPath := filepath.Join("..", "..", "..", "..", "infra", "postgres", "migrations", wikiOutboxMigration)
	outboxMigration, err := os.ReadFile(outboxMigrationPath)
	if err != nil {
		t.Fatalf("read wiki outbox migration: %v", err)
	}
	if _, err := pool.Exec(ctx, string(outboxMigration)); err != nil {
		t.Fatalf("apply wiki outbox migration: %v", err)
	}
	if _, err := pool.Exec(ctx, triggerSetUpdatedAtFn); err != nil {
		t.Fatalf("create trigger_set_updated_at helper: %v", err)
	}
	operatingMapsMigrationPath := filepath.Join("..", "..", "..", "..", "infra", "postgres", "migrations", operatingMapsMigration)
	operatingMapsSQL, err := os.ReadFile(operatingMapsMigrationPath)
	if err != nil {
		t.Fatalf("read operating maps migration: %v", err)
	}
	if _, err := pool.Exec(ctx, string(operatingMapsSQL)); err != nil {
		t.Fatalf("apply operating maps migration: %v", err)
	}
	return pool
}

func countRowsForOrg(ctx context.Context, t *testing.T, pool *pgxpool.Pool, table, orgID string) int {
	t.Helper()
	var n int
	query := "SELECT COUNT(*) FROM " + table + " WHERE org_id = $1"
	if err := pool.QueryRow(ctx, query, orgID).Scan(&n); err != nil {
		t.Fatalf("count %s for %s: %v", table, orgID, err)
	}
	return n
}

// countCascadeRowsForOrg counts rows in a table with no org_id column of its
// own, joining back to whichever parent table (wiki_pages or operating_maps)
// actually carries org_id.
func countCascadeRowsForOrg(ctx context.Context, t *testing.T, pool *pgxpool.Pool, table, orgID string) int {
	t.Helper()
	var query string
	switch table {
	case "wiki_page_versions":
		query = `SELECT COUNT(*) FROM wiki_page_versions v JOIN wiki_pages p ON v.page_id = p.page_id WHERE p.org_id = $1`
	case "wiki_proposals":
		query = `SELECT COUNT(*) FROM wiki_proposals WHERE org_id = $1`
	case "operating_map_versions":
		query = `SELECT COUNT(*) FROM operating_map_versions WHERE org_id = $1`
	case "operating_map_proposals":
		query = `SELECT COUNT(*) FROM operating_map_proposals WHERE org_id = $1`
	case "operating_map_blueprint_suggestions":
		query = `SELECT COUNT(*) FROM operating_map_blueprint_suggestions WHERE org_id = $1`
	default:
		t.Fatalf("countCascadeRowsForOrg: unhandled table %q", table)
	}
	var n int
	if err := pool.QueryRow(ctx, query, orgID).Scan(&n); err != nil {
		t.Fatalf("count %s for %s: %v", table, orgID, err)
	}
	return n
}

// seedOrg inserts one row into every purge-scoped and cascade-purged table
// for orgID, using pageID/mapID/versionID as the shared keys those rows hang
// off of.
func seedOrg(ctx context.Context, t *testing.T, pool *pgxpool.Pool, orgID, pageID, mapID, mapVersionID string) {
	t.Helper()
	if _, err := pool.Exec(ctx, `
		INSERT INTO wiki_pages (page_id, org_id, workspace_id, title, path)
		VALUES ($1, $2, 'workspace-1', 'Title', '/'||$1);
	`, pageID, orgID); err != nil {
		t.Fatalf("seed wiki_pages for %s: %v", orgID, err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO wiki_page_versions (version_id, page_id, content, version_status)
		VALUES ($1, $2, 'secret content', 'published');
	`, "version-"+pageID, pageID); err != nil {
		t.Fatalf("seed wiki_page_versions for %s: %v", orgID, err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO wiki_proposals (proposal_id, page_id, org_id, proposed_content)
		VALUES ($1, $2, $3, 'proposed content');
	`, "proposal-"+pageID, pageID, orgID); err != nil {
		t.Fatalf("seed wiki_proposals for %s: %v", orgID, err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO wiki_source_logs (log_id, org_id, page_id, source_type, source_ref, sync_status, details)
		VALUES ($1, $2, $3, 'document', 'ref', 'synced', '{}'::jsonb);
	`, "source-"+pageID, orgID, pageID); err != nil {
		t.Fatalf("seed wiki_source_logs for %s: %v", orgID, err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO wiki_maintenance_logs (log_id, org_id, page_id, action, actor, details)
		VALUES ($1, $2, $3, 'flagged', 'tester', '{}'::jsonb);
	`, "maintenance-"+pageID, orgID, pageID); err != nil {
		t.Fatalf("seed wiki_maintenance_logs for %s: %v", orgID, err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO wiki_event_outbox (org_id, event_type, payload, idempotency_key, status)
		VALUES ($1::text, 'dataplane.wiki.version.published',
			jsonb_build_object('org_id', $1::text, 'zdr', 'false', 'page_id', $2::text, 'version_id', $3::text, 'content', 'c'),
			$4::text, 'pending');
	`, orgID, pageID, "version-"+pageID, "wiki.published:version-"+pageID); err != nil {
		t.Fatalf("seed wiki_event_outbox for %s: %v", orgID, err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO operating_maps (operating_map_id, org_id, map_status)
		VALUES ($1, $2, 'draft');
	`, mapID, orgID); err != nil {
		t.Fatalf("seed operating_maps for %s: %v", orgID, err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO operating_map_versions (version_id, operating_map_id, org_id)
		VALUES ($1, $2, $3);
	`, mapVersionID, mapID, orgID); err != nil {
		t.Fatalf("seed operating_map_versions for %s: %v", orgID, err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO operating_map_proposals (proposal_id, operating_map_id, org_id, proposed_version)
		VALUES ($1, $2, $3, '{}'::jsonb);
	`, "map-proposal-"+mapID, mapID, orgID); err != nil {
		t.Fatalf("seed operating_map_proposals for %s: %v", orgID, err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO operating_map_blueprint_suggestions
			(suggestion_id, operating_map_id, version_id, org_id, blueprint_id, role, name)
		VALUES ($1, $2, $3, $4, 'blueprint-1', 'service', 'Suggested Blueprint');
	`, "suggestion-"+mapID, mapID, mapVersionID, orgID); err != nil {
		t.Fatalf("seed operating_map_blueprint_suggestions for %s: %v", orgID, err)
	}
}

func TestHardPurgeByOrgIsOrgScopedAndIdempotent(t *testing.T) {
	pool := setupPurgeTestPool(t)
	ctx := context.Background()
	wikiRepo := NewWikiRepo(pool)

	seedOrg(ctx, t, pool, "org-A", "page-a", "map-a", "map-a-version-1")
	seedOrg(ctx, t, pool, "org-B", "page-b", "map-b", "map-b-version-1")

	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-A"); n == 0 {
			t.Fatalf("fixture bug: org-A has no seed rows in %s", table)
		}
		if n := countRowsForOrg(ctx, t, pool, table, "org-B"); n == 0 {
			t.Fatalf("fixture bug: org-B has no seed rows in %s", table)
		}
	}
	for _, table := range cascadePurgedTables {
		if n := countCascadeRowsForOrg(ctx, t, pool, table, "org-A"); n == 0 {
			t.Fatalf("fixture bug: org-A has no seed rows in %s", table)
		}
		if n := countCascadeRowsForOrg(ctx, t, pool, table, "org-B"); n == 0 {
			t.Fatalf("fixture bug: org-B has no seed rows in %s", table)
		}
	}

	result, err := wikiRepo.HardPurgeByOrg(ctx, "org-A")
	if err != nil {
		t.Fatalf("hard purge org-A: %v", err)
	}
	if result.Total() == 0 {
		t.Fatalf("expected a nonzero purge result for a seeded org, got %+v", result)
	}

	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-A"); n != 0 {
			t.Fatalf("org-A %s not purged: %d rows remain", table, n)
		}
	}
	for _, table := range cascadePurgedTables {
		if n := countCascadeRowsForOrg(ctx, t, pool, table, "org-A"); n != 0 {
			t.Fatalf("org-A %s not cascade-purged: %d rows remain", table, n)
		}
	}

	// The other org's data — created in the very same test run — must be
	// completely unaffected by org-A's purge.
	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-B"); n == 0 {
			t.Fatalf("org-B %s was wrongly purged by org-A's erasure", table)
		}
	}
	for _, table := range cascadePurgedTables {
		if n := countCascadeRowsForOrg(ctx, t, pool, table, "org-B"); n == 0 {
			t.Fatalf("org-B %s was wrongly cascade-purged by org-A's erasure", table)
		}
	}

	// NATS is at-least-once delivery: the consumer may redeliver and re-run
	// the purge for an org that was already fully purged. That must not
	// error, must return an all-zero result, and must still leave org-B
	// untouched.
	second, err := wikiRepo.HardPurgeByOrg(ctx, "org-A")
	if err != nil {
		t.Fatalf("second hard purge of org-A must be idempotent, got error: %v", err)
	}
	if second.Total() != 0 {
		t.Fatalf("second purge of an already-purged org must delete nothing, got %+v", second)
	}
	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-B"); n == 0 {
			t.Fatalf("org-B %s was wrongly purged by org-A's redelivered erasure", table)
		}
	}
}

func TestHardPurgeByOrgRejectsEmptyOrgID(t *testing.T) {
	pool := setupPurgeTestPool(t)
	wikiRepo := NewWikiRepo(pool)
	if _, err := wikiRepo.HardPurgeByOrg(context.Background(), "   "); err == nil {
		t.Fatal("expected an error for a blank org_id — must never resolve to an unscoped DELETE")
	}
}

package gdpr

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// purgeScopedTables lists every table HardPurgeByOrg deletes from — kept in
// one place so the "org A fully purged" / "org B untouched" assertions below
// loop over the exact same list HardPurgeByOrg's implementation uses.
var purgeScopedTables = []string{"data_orchestrator_jobs", "cost_events"}

func countRowsForOrg(ctx context.Context, t *testing.T, pool *pgxpool.Pool, table, orgID string) int {
	t.Helper()
	var n int
	query := fmt.Sprintf(`SELECT COUNT(*) FROM %s WHERE org_id = $1`, table)
	if err := pool.QueryRow(ctx, query, orgID).Scan(&n); err != nil {
		t.Fatalf("count %s for %s: %v", table, orgID, err)
	}
	return n
}

// TestHardPurgeByOrgIsOrgScopedAndIdempotent is the safety contract for the
// GDPR org-erasure consumer (org_purge.go): purging org A must never touch
// org B's rows created in the same run, and redelivering the same purge
// (NATS is at-least-once) must not error the second time.
func TestHardPurgeByOrgIsOrgScopedAndIdempotent(t *testing.T) {
	pool, cleanup := setupPurgeTestPostgres(t)
	defer cleanup()

	repo := NewPurgeRepo(pool)
	ctx := context.Background()

	// Seed org A: a data_orchestrator_jobs row and a cost_events row.
	if _, err := pool.Exec(ctx, `
		INSERT INTO data_orchestrator_jobs (job_id, org_id, job_type, status, idempotency_key, created_at, updated_at)
		VALUES ($1::uuid, 'org-A', 'reindex', 'pending', 'purge-test-a-job', NOW(), NOW())
	`, uuid.NewString()); err != nil {
		t.Fatalf("seed org-A job: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key)
		VALUES ('embedding', 'test-model', 'org-A', 10, 1000, 'purge-test-a-cost')
	`); err != nil {
		t.Fatalf("seed org-A cost event: %v", err)
	}

	// Seed org B with equivalent rows — the control group that must survive
	// org A's purge untouched.
	if _, err := pool.Exec(ctx, `
		INSERT INTO data_orchestrator_jobs (job_id, org_id, job_type, status, idempotency_key, created_at, updated_at)
		VALUES ($1::uuid, 'org-B', 'reindex', 'pending', 'purge-test-b-job', NOW(), NOW())
	`, uuid.NewString()); err != nil {
		t.Fatalf("seed org-B job: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key)
		VALUES ('embedding', 'test-model', 'org-B', 10, 1000, 'purge-test-b-cost')
	`); err != nil {
		t.Fatalf("seed org-B cost event: %v", err)
	}

	// Sanity: both orgs have rows in every scoped table before the purge.
	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-A"); n == 0 {
			t.Fatalf("fixture bug: org-A has no seed rows in %s", table)
		}
		if n := countRowsForOrg(ctx, t, pool, table, "org-B"); n == 0 {
			t.Fatalf("fixture bug: org-B has no seed rows in %s", table)
		}
	}

	if err := repo.HardPurgeByOrg(ctx, "org-A"); err != nil {
		t.Fatalf("hard purge org-A: %v", err)
	}

	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-A"); n != 0 {
			t.Fatalf("org-A %s not purged: %d rows remain", table, n)
		}
	}
	// The other org's data — created in the very same test run — must be
	// completely unaffected by org-A's purge.
	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-B"); n == 0 {
			t.Fatalf("org-B %s was wrongly purged by org-A's erasure", table)
		}
	}

	// NATS is at-least-once delivery: the consumer may redeliver and re-run
	// the purge for an org that was already fully purged. That must not
	// error, and must still leave org B untouched.
	if err := repo.HardPurgeByOrg(ctx, "org-A"); err != nil {
		t.Fatalf("second hard purge of org-A must be idempotent, got error: %v", err)
	}
	for _, table := range purgeScopedTables {
		if n := countRowsForOrg(ctx, t, pool, table, "org-B"); n == 0 {
			t.Fatalf("org-B %s was wrongly purged by org-A's redelivered erasure", table)
		}
	}
}

func TestHardPurgeByOrgRejectsEmptyOrgID(t *testing.T) {
	pool, cleanup := setupPurgeTestPostgres(t)
	defer cleanup()

	repo := NewPurgeRepo(pool)
	if err := repo.HardPurgeByOrg(context.Background(), "   "); err == nil {
		t.Fatal("expected an error for a blank org_id — must never resolve to an unscoped DELETE")
	}
}

// setupPurgeTestPostgres mirrors internal/jobs/store_integration_test.go's
// orchestratorIntegrationPool: a disposable schema on TEST_DATABASE_URL, torn
// down (CASCADE) on cleanup. Applies the same migrations production runs
// (cost_events + its user_id follow-up, plus the durability migration that
// creates data_orchestrator_jobs) so this exercises the exact schema
// PurgeRepo queries in production.
func setupPurgeTestPostgres(t *testing.T) (*pgxpool.Pool, func()) {
	t.Helper()
	databaseURL := os.Getenv("TEST_DATABASE_URL")
	if databaseURL == "" {
		t.Skip("TEST_DATABASE_URL is required for disposable PostgreSQL integration tests")
	}
	ctx := context.Background()
	adminConfig, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse TEST_DATABASE_URL: %v", err)
	}
	adminConfig.ConnConfig.DefaultQueryExecMode = pgx.QueryExecModeSimpleProtocol
	admin, err := pgxpool.NewWithConfig(ctx, adminConfig)
	if err != nil {
		t.Fatalf("connect disposable PostgreSQL: %v", err)
	}
	schema := "gdpr_org_purge_" + strings.ReplaceAll(uuid.NewString(), "-", "")
	identifier := pgx.Identifier{schema}.Sanitize()
	if _, err := admin.Exec(ctx, "CREATE SCHEMA "+identifier); err != nil {
		admin.Close()
		t.Fatalf("create disposable schema: %v", err)
	}

	config, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse schema config: %v", err)
	}
	config.ConnConfig.DefaultQueryExecMode = pgx.QueryExecModeSimpleProtocol
	config.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, config)
	if err != nil {
		t.Fatalf("connect disposable schema: %v", err)
	}

	for _, migration := range []string{
		"20260508140000_add_cost_events.sql",
		"20260508180000_add_user_id_to_cost_events.sql",
		"20260711160000_quality_orchestrator_durability.sql",
	} {
		if _, err := pool.Exec(ctx, readMigration(t, migration)); err != nil {
			t.Fatalf("apply migration %s: %v", migration, err)
		}
	}

	cleanup := func() {
		pool.Close()
		if _, err := admin.Exec(context.Background(), "DROP SCHEMA "+identifier+" CASCADE"); err != nil {
			t.Errorf("drop disposable schema: %v", err)
		}
		admin.Close()
	}
	return pool, cleanup
}

func readMigration(t *testing.T, name string) string {
	t.Helper()
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("locate integration test source")
	}
	path := filepath.Join(filepath.Dir(source), "..", "..", "..", "..", "infra", "postgres", "migrations", name)
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read migration %s: %v", name, err)
	}
	return string(contents)
}

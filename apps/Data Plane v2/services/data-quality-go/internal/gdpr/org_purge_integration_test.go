package gdpr

import (
	"context"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// TestPostgresOrgPurger_HardPurgeByOrg_IsolatesOtherOrgs seeds two
// organizations' rows across both tables this service owns
// (quality_eval_runs, eval_golden_judgments), purges org A, and asserts every
// org B row survives untouched — the safety contract's required isolation
// test (org A's purge must never be able to touch org B's rows).
func TestPostgresOrgPurger_HardPurgeByOrg_IsolatesOtherOrgs(t *testing.T) {
	pool := gdprIntegrationPool(t)
	purger := NewPostgresOrgPurger(pool)
	ctx := context.Background()

	const orgA = "org-gdpr-purge-a"
	const orgB = "org-gdpr-purge-b"
	seedQualityEvalRun(t, pool, orgA, "quality-eval-purge-a-1")
	seedQualityEvalRun(t, pool, orgB, "quality-eval-purge-b-1")
	seedGoldenJudgment(t, pool, orgA, "query one")
	seedGoldenJudgment(t, pool, orgB, "query one")

	if err := purger.HardPurgeByOrg(ctx, orgA); err != nil {
		t.Fatalf("HardPurgeByOrg(org A): %v", err)
	}

	if n := countRows(t, pool, "quality_eval_runs", orgA); n != 0 {
		t.Fatalf("quality_eval_runs rows for purged org A = %d, want 0", n)
	}
	if n := countRows(t, pool, "eval_golden_judgments", orgA); n != 0 {
		t.Fatalf("eval_golden_judgments rows for purged org A = %d, want 0", n)
	}
	if n := countRows(t, pool, "quality_eval_runs", orgB); n != 1 {
		t.Fatalf("quality_eval_runs rows for untouched org B = %d, want 1 (org B must survive org A's purge)", n)
	}
	if n := countRows(t, pool, "eval_golden_judgments", orgB); n != 1 {
		t.Fatalf("eval_golden_judgments rows for untouched org B = %d, want 1 (org B must survive org A's purge)", n)
	}

	// Idempotency: NATS is at-least-once delivery — a redelivered purge for
	// the already-purged org A must match zero rows and return nil, not error.
	if err := purger.HardPurgeByOrg(ctx, orgA); err != nil {
		t.Fatalf("redelivered HardPurgeByOrg(org A) returned error: %v", err)
	}
}

func TestPostgresOrgPurger_HardPurgeByOrg_RejectsEmptyOrgID(t *testing.T) {
	pool := gdprIntegrationPool(t)
	purger := NewPostgresOrgPurger(pool)
	if err := purger.HardPurgeByOrg(context.Background(), "   "); err == nil {
		t.Fatal("expected an error for a blank org_id")
	}
}

func seedQualityEvalRun(t *testing.T, pool *pgxpool.Pool, orgID, idempotencyKey string) {
	t.Helper()
	_, err := pool.Exec(context.Background(), `
		INSERT INTO quality_eval_runs (eval_id, org_id, strategy, corpus, status, idempotency_key)
		VALUES ($1::uuid, $2, 'hybrid', '', 'pending', $3)
	`, uuid.NewString(), orgID, idempotencyKey)
	if err != nil {
		t.Fatalf("seed quality_eval_runs (org=%s): %v", orgID, err)
	}
}

func seedGoldenJudgment(t *testing.T, pool *pgxpool.Pool, orgID, query string) {
	t.Helper()
	_, err := pool.Exec(context.Background(), `
		INSERT INTO eval_golden_judgments (org_id, query_norm, relevant_ids)
		VALUES ($1, $2, '["doc-1"]'::jsonb)
	`, orgID, query)
	if err != nil {
		t.Fatalf("seed eval_golden_judgments (org=%s): %v", orgID, err)
	}
}

func countRows(t *testing.T, pool *pgxpool.Pool, table, orgID string) int {
	t.Helper()
	identifier := pgx.Identifier{table}.Sanitize()
	var n int
	if err := pool.QueryRow(context.Background(), "SELECT count(*) FROM "+identifier+" WHERE org_id = $1", orgID).Scan(&n); err != nil {
		t.Fatalf("count %s rows (org=%s): %v", table, orgID, err)
	}
	return n
}

// gdprIntegrationPool mirrors internal/eval's qualityIntegrationPool: a
// disposable schema seeded with both migrations this package's purge targets
// depend on.
func gdprIntegrationPool(t *testing.T) *pgxpool.Pool {
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
	schema := "quality_gdpr_" + strings.ReplaceAll(uuid.NewString(), "-", "")
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

	durabilityUp := readMigration(t, "20260711160000_quality_orchestrator_durability.sql")
	if _, err := pool.Exec(ctx, durabilityUp); err != nil {
		t.Fatalf("apply durability migration: %v", err)
	}
	goldenUp := readMigration(t, "20260717100000_eval_golden_judgments.sql")
	if _, err := pool.Exec(ctx, goldenUp); err != nil {
		t.Fatalf("apply golden-judgments migration: %v", err)
	}

	t.Cleanup(func() {
		goldenDown := readMigration(t, "20260717100000_eval_golden_judgments.down.sql")
		if _, err := pool.Exec(context.Background(), goldenDown); err != nil {
			t.Errorf("apply golden-judgments rollback: %v", err)
		}
		durabilityDown := readMigration(t, "20260711160000_quality_orchestrator_durability.down.sql")
		if _, err := pool.Exec(context.Background(), durabilityDown); err != nil {
			t.Errorf("apply durability rollback: %v", err)
		}
		pool.Close()
		if _, err := admin.Exec(context.Background(), "DROP SCHEMA "+identifier+" CASCADE"); err != nil {
			t.Errorf("drop disposable schema: %v", err)
		}
		admin.Close()
	})
	return pool
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

package jobs

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

func TestReconcileMaxAttemptsFallsBackToTheRustDefault(t *testing.T) {
	t.Setenv("EMBEDDING_RECONCILE_MAX_ATTEMPTS", "")
	if got := reconcileMaxAttempts(); got != defaultReconcileMaxAttempts {
		t.Fatalf("unset: got %d, want %d", got, defaultReconcileMaxAttempts)
	}

	// A garbage or negative override must not silently widen (or invert) the
	// ceiling this report is judged against.
	for _, raw := range []string{"not-a-number", "-1", " "} {
		t.Setenv("EMBEDDING_RECONCILE_MAX_ATTEMPTS", raw)
		if got := reconcileMaxAttempts(); got != defaultReconcileMaxAttempts {
			t.Fatalf("%q: got %d, want %d", raw, got, defaultReconcileMaxAttempts)
		}
	}

	t.Setenv("EMBEDDING_RECONCILE_MAX_ATTEMPTS", "3")
	if got := reconcileMaxAttempts(); got != 3 {
		t.Fatalf("override: got %d, want 3", got)
	}
}

// TestStaleDetectorSeparatesRetryableFromExhaustedFailures covers the two
// behaviours D19 changes in this file: the failed bucket splits by whether the
// index-engine reconciler will still re-drive the unit, and a freshly
// re-driven unit stops being mis-reported as stuck-pending.
func TestStaleDetectorSeparatesRetryableFromExhaustedFailures(t *testing.T) {
	pool := staleDetectorPool(t)
	ctx := context.Background()

	// One org, six units, so every branch is exercised at once.
	//   doc-retry     : failed, 1 of 5 attempts used  -> retryable
	//   doc-exhausted : failed, 5 of 5 attempts used  -> exhausted
	//   doc-fresh     : pending, chunked long ago, just re-driven -> NOT stuck
	//   doc-stuck     : pending, chunked long ago, never re-driven -> stuck
	seed := `
		INSERT INTO documents (document_id, org_id) VALUES
			('doc-retry', 'org-a'), ('doc-exhausted', 'org-a'),
			('doc-fresh', 'org-a'), ('doc-stuck', 'org-a'),
			('doc-other-org', 'org-b');
		INSERT INTO knowledge_units
			(knowledge_id, document_id, org_id, embedding_status,
			 embedding_retry_count, embedding_retry_at, created_at, updated_at) VALUES
			('k-retry',     'doc-retry',     'org-a', 'failed',  1, NOW() - INTERVAL '2 hours',
			 NOW() - INTERVAL '3 days', NOW() - INTERVAL '2 hours'),
			('k-exhausted', 'doc-exhausted', 'org-a', 'failed',  5, NOW() - INTERVAL '2 hours',
			 NOW() - INTERVAL '3 days', NOW() - INTERVAL '2 hours'),
			('k-fresh',     'doc-fresh',     'org-a', 'pending', 1, NOW(),
			 NOW() - INTERVAL '3 days', NOW()),
			('k-stuck',     'doc-stuck',     'org-a', 'pending', 0, NULL,
			 NOW() - INTERVAL '3 days', NOW() - INTERVAL '3 days'),
			('k-other',     'doc-other-org', 'org-b', 'failed',  9, NULL,
			 NOW() - INTERVAL '3 days', NOW() - INTERVAL '3 days');
	`
	if _, err := pool.Exec(ctx, seed); err != nil {
		t.Fatalf("seed: %v", err)
	}

	report, err := NewStaleDetector(pool).Detect(ctx, "org-a", 30*time.Minute)
	if err != nil {
		t.Fatalf("detect: %v", err)
	}

	if report.FailedCount != 2 {
		t.Errorf("failed_count = %d, want 2", report.FailedCount)
	}
	if report.RetryableFailedCount != 1 {
		t.Errorf("retryable_failed_count = %d, want 1 (doc-retry)", report.RetryableFailedCount)
	}
	if report.ExhaustedFailedCount != 1 {
		t.Errorf("exhausted_failed_count = %d, want 1 (doc-exhausted)", report.ExhaustedFailedCount)
	}
	if len(report.ExhaustedFailedDocumentIDs) != 1 || report.ExhaustedFailedDocumentIDs[0] != "doc-exhausted" {
		t.Errorf("exhausted ids = %v, want [doc-exhausted]", report.ExhaustedFailedDocumentIDs)
	}

	// The regression this guards: a re-driven unit keeps its original
	// created_at, so before the embedding_retry_at clause it would have been
	// reported stuck the instant the reconciler touched it.
	if report.StuckPendingCount != 1 {
		t.Errorf("stuck_pending_count = %d, want 1 (doc-stuck only)", report.StuckPendingCount)
	}
	for _, id := range report.StuckDocumentIDs {
		if id == "doc-fresh" {
			t.Error("a just-re-driven unit was reported as stuck-pending")
		}
	}

	// Tenant isolation: org-b's exhausted unit must not leak into org-a.
	if got := strings.Join(report.FailedDocumentIDs, ","); strings.Contains(got, "doc-other-org") {
		t.Errorf("cross-tenant leak in failed ids: %s", got)
	}
}

// staleDetectorPool builds a disposable schema holding the minimum shape the
// detector reads, then applies the REAL D19 migration on top so the test
// proves the shipped SQL both works and is idempotent.
func staleDetectorPool(t *testing.T) *pgxpool.Pool {
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
	schema := "stale_detector_" + strings.ReplaceAll(uuid.NewString(), "-", "")
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

	base := `
		CREATE TABLE documents (
			document_id TEXT PRIMARY KEY,
			org_id      TEXT NOT NULL,
			deleted_at  TIMESTAMPTZ
		);
		CREATE TABLE knowledge_units (
			knowledge_id     TEXT PRIMARY KEY,
			document_id      TEXT NOT NULL REFERENCES documents(document_id),
			org_id           TEXT NOT NULL,
			embedding_status TEXT NOT NULL DEFAULT 'pending',
			created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
			updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
			embedded_at      TIMESTAMPTZ
		);
	`
	if _, err := pool.Exec(ctx, base); err != nil {
		t.Fatalf("create base schema: %v", err)
	}

	up := readDurabilityMigration(t, "20260807120000_embedding_retry_bookkeeping.sql")
	if _, err := pool.Exec(ctx, up); err != nil {
		t.Fatalf("apply retry-bookkeeping migration: %v", err)
	}
	if _, err := pool.Exec(ctx, up); err != nil {
		t.Fatalf("reapply idempotent retry-bookkeeping migration: %v", err)
	}

	t.Cleanup(func() {
		down := readDurabilityMigration(t, "20260807120000_embedding_retry_bookkeeping.down.sql")
		if _, err := pool.Exec(context.Background(), down); err != nil {
			t.Errorf("apply retry-bookkeeping rollback: %v", err)
		}
		pool.Close()
		if _, err := admin.Exec(context.Background(), "DROP SCHEMA "+identifier+" CASCADE"); err != nil {
			t.Errorf("drop disposable schema: %v", err)
		}
		admin.Close()
	})
	return pool
}

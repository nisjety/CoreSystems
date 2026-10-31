package db_test

import (
	"context"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// Several replicas starting together must not race on schema_migrations. Each
// round starts from a clean database, so the test drops schema_migrations and the
// raw-store objects. That is safe because dbtest gives it a database of its own.
func TestApplyMigrationsIsSafeWhenSeveralInstancesStartTogether(t *testing.T) {
	pool := dbtest.Open(t)
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()

	const instances = 8
	for round := range 15 {
		// Migrations 0002 to 0007 are plain DDL with no IF NOT EXISTS, so each round
		// drops their objects too. Tables go first: their triggers call the guard
		// functions, and dropping a table drops its triggers and the columns and
		// constraints that migrations 0004 and 0006 changed. CASCADE also drops the view
		// raw_documents_live of 0006, which depends on raw_documents. The tombstone table
		// of 0006 is named, because it only references raw_documents.
		if _, err := pool.Exec(ctx, `DROP TABLE IF EXISTS catalogue_projection, catalogue_generation, raw_redactions, download_attempts, sync_days, raw_documents, raw_search_pages, sync_runs CASCADE`); err != nil {
			t.Fatalf("reset raw store tables: %v", err)
		}
		if _, err := pool.Exec(ctx, `DROP FUNCTION IF EXISTS raw_append_only_guard()`); err != nil {
			t.Fatalf("reset raw store guard: %v", err)
		}
		if _, err := pool.Exec(ctx, `DROP FUNCTION IF EXISTS sync_runs_provenance_guard()`); err != nil {
			t.Fatalf("reset run provenance guard: %v", err)
		}
		if _, err := pool.Exec(ctx, `DROP FUNCTION IF EXISTS evidence_run_guard()`); err != nil {
			t.Fatalf("reset evidence run guard: %v", err)
		}
		for _, fn := range []string{"anbud_redact_raw_document(bigint, text, text)", "raw_documents_change_guard()", "raw_documents_insert_guard()", "catalogue_source_changed()", "catalogue_projection_source_guard()"} {
			if _, err := pool.Exec(ctx, `DROP FUNCTION IF EXISTS `+fn); err != nil {
				t.Fatalf("reset redaction function %s: %v", fn, err)
			}
		}
		// Migration 0007 creates a schema of its own, with the table and the function in it.
		if _, err := pool.Exec(ctx, `DROP SCHEMA IF EXISTS tenant CASCADE`); err != nil {
			t.Fatalf("reset the tenant schema: %v", err)
		}
		if _, err := pool.Exec(ctx, `DROP TABLE IF EXISTS schema_migrations`); err != nil {
			t.Fatalf("reset: %v", err)
		}

		start := make(chan struct{})
		errs := make(chan error, instances)
		var wg sync.WaitGroup
		for range instances {
			wg.Go(func() {
				<-start
				errs <- db.ApplyMigrations(ctx, pool)
			})
		}
		close(start)
		wg.Wait()
		close(errs)

		for err := range errs {
			if err != nil {
				t.Fatalf("round %d: a concurrent ApplyMigrations failed: %v", round+1, err)
			}
		}

		for _, version := range []int{1, 2, 3, 4, 5, 6} {
			var applied int
			if err := pool.QueryRow(ctx, `SELECT count(*) FROM schema_migrations WHERE version = $1`, version).Scan(&applied); err != nil {
				t.Fatalf("round %d: read schema_migrations for %d: %v", round+1, version, err)
			}
			if applied != 1 {
				t.Fatalf("round %d: migration %d recorded %d times, want exactly once", round+1, version, applied)
			}
		}
	}
}

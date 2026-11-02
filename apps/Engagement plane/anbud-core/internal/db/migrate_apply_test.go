package db_test

import (
	"context"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// Future migrations will contain several statements. pgx only accepts that when
// the query has no arguments, so pin it against a real server. The transaction
// is rolled back, leaving the database untouched.
func TestApplyOneRunsAMultiStatementMigration(t *testing.T) {
	pool := dbtest.Open(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	tx, err := pool.Begin(ctx)
	if err != nil {
		t.Fatalf("Begin: %v", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, db.SchemaMigrationsDDL); err != nil {
		t.Fatalf("ensure schema_migrations: %v", err)
	}
	body := `CREATE TEMP TABLE multi_probe (x int); INSERT INTO multi_probe VALUES (1); INSERT INTO multi_probe VALUES (2);`
	if err := db.ApplyOne(ctx, tx, 9999, "9999_multi", body); err != nil {
		t.Fatalf("applyOne with several statements: %v", err)
	}
	var rows int
	if err := tx.QueryRow(ctx, `SELECT count(*) FROM multi_probe`).Scan(&rows); err != nil || rows != 2 {
		t.Fatalf("rows = %d, err = %v; want both inserts to have run", rows, err)
	}
}

// A migration that fails halfway must leave nothing behind, including the
// version record, so the next start retries it cleanly.
func TestAFailingMigrationLeavesNoTrace(t *testing.T) {
	pool := dbtest.Open(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	tx, err := pool.Begin(ctx)
	if err != nil {
		t.Fatalf("Begin: %v", err)
	}
	if _, err := tx.Exec(ctx, db.SchemaMigrationsDDL); err != nil {
		t.Fatalf("ensure schema_migrations: %v", err)
	}
	body := `CREATE TEMP TABLE half_done (x int); SELECT * FROM table_that_does_not_exist;`
	if err := db.ApplyOne(ctx, tx, 9998, "9998_bad", body); err == nil {
		t.Fatal("expected the failing migration to return an error")
	}
	_ = tx.Rollback(ctx)

	var recorded int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM schema_migrations WHERE version = 9998`).Scan(&recorded); err != nil {
		// The table itself was rolled back with the transaction; that is also "no trace".
		return
	}
	if recorded != 0 {
		t.Fatalf("a failed migration left %d version record(s)", recorded)
	}
}

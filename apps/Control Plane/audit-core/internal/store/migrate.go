package store

import (
	"context"
	_ "embed"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

//go:embed schema.sql
var schemaSQL string

// schemaVersion is the schema_migrations ledger key for the bundled baseline
// schema. audit-core ships a single embedded schema file rather than a
// migrations directory, so the ledger carries one baseline row; future schema
// changes add new versioned entries alongside it.
const schemaVersion = "001_init"

// Migrate applies the bundled SQL schema exactly once, tracking it in a
// schema_migrations ledger (the Data Plane v2 migrator pattern, ported per
// Phase 6 B4). Previously the idempotent DDL was re-executed on every boot;
// now the embedded baseline is applied inside a transaction, recorded, and
// skipped on subsequent boots.
//
// The DDL is idempotent (`CREATE TABLE/INDEX IF NOT EXISTS`), so applying it
// against an already-populated pre-ledger database is safe even though we then
// record it as applied — the schema simply matches and the row is written once.
func Migrate(ctx context.Context, pool *pgxpool.Pool) error {
	if _, err := pool.Exec(ctx, `
		CREATE TABLE IF NOT EXISTS schema_migrations (
			version     TEXT        PRIMARY KEY,
			name        TEXT        NOT NULL DEFAULT '',
			applied_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
			duration_ms INTEGER     NOT NULL DEFAULT 0
		)`); err != nil {
		return fmt.Errorf("create schema_migrations: %w", err)
	}

	var applied bool
	if err := pool.QueryRow(ctx,
		"SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = $1)", schemaVersion,
	).Scan(&applied); err != nil {
		return fmt.Errorf("check schema migration %s: %w", schemaVersion, err)
	}
	if applied {
		return nil
	}

	start := time.Now()
	tx, err := pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin schema tx: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }() // no-op after a successful Commit

	if _, err := tx.Exec(ctx, schemaSQL); err != nil {
		return fmt.Errorf("apply schema: %w", err)
	}
	if _, err := tx.Exec(ctx,
		"INSERT INTO schema_migrations (version, name, duration_ms) VALUES ($1, $2, $3)",
		schemaVersion, schemaVersion, int(time.Since(start).Milliseconds()),
	); err != nil {
		return fmt.Errorf("record schema migration %s: %w", schemaVersion, err)
	}

	return tx.Commit(ctx)
}

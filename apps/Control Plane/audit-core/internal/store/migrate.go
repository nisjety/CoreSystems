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

//go:embed migrations/002_jetstream_inbox.sql
var jetStreamInboxSQL string

//go:embed migrations/003_logical_control_audit_idempotency.sql
var logicalControlAuditIdempotencySQL string

//go:embed migrations/004_logical_usage_idempotency.sql
var logicalUsageIdempotencySQL string

//go:embed migrations/005_usage_source_identity.sql
var usageSourceIdentitySQL string

//go:embed migrations/006_v2_producer_authority.sql
var v2ProducerAuthoritySQL string

type migration struct {
	version string
	name    string
	sql     string
}

var migrations = []migration{
	{version: "001_init", name: "initial audit schema", sql: schemaSQL},
	{version: "002_jetstream_inbox", name: "JetStream inbox idempotency", sql: jetStreamInboxSQL},
	{version: "003_logical_control_audit_idempotency", name: "logical Control audit idempotency", sql: logicalControlAuditIdempotencySQL},
	{version: "004_logical_usage_idempotency", name: "logical usage idempotency", sql: logicalUsageIdempotencySQL},
	{version: "005_usage_source_identity", name: "usage producer identity", sql: usageSourceIdentitySQL},
	{version: "006_v2_producer_authority", name: "v2 producer authority identity", sql: v2ProducerAuthoritySQL},
}

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

	for _, item := range migrations {
		if err := applyMigration(ctx, pool, item); err != nil {
			return err
		}
	}
	return nil
}

func applyMigration(ctx context.Context, pool *pgxpool.Pool, item migration) error {
	var applied bool
	if err := pool.QueryRow(ctx,
		"SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = $1)", item.version,
	).Scan(&applied); err != nil {
		return fmt.Errorf("check schema migration %s: %w", item.version, err)
	}
	if applied {
		return nil
	}

	start := time.Now()
	tx, err := pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin schema migration %s: %w", item.version, err)
	}
	defer func() { _ = tx.Rollback(ctx) }() // no-op after a successful Commit

	if _, err := tx.Exec(ctx, item.sql); err != nil {
		return fmt.Errorf("apply schema migration %s: %w", item.version, err)
	}
	if _, err := tx.Exec(ctx,
		"INSERT INTO schema_migrations (version, name, duration_ms) VALUES ($1, $2, $3)",
		item.version, item.name, int(time.Since(start).Milliseconds()),
	); err != nil {
		return fmt.Errorf("record schema migration %s: %w", item.version, err)
	}

	return tx.Commit(ctx)
}

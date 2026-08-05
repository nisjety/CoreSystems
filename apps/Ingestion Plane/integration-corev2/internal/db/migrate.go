package db

import (
	"context"
	"embed"
	"fmt"
	"path/filepath"
	"sort"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

//go:embed migrations/*.sql
var migrationFS embed.FS

type migrationPool interface {
	Begin(context.Context) (pgx.Tx, error)
}

type migrationQuerier interface {
	QueryRow(context.Context, string, ...any) pgx.Row
}

const migrationAdvisoryLockSQL = `SELECT pg_advisory_xact_lock(hashtext('verevon'), hashtext('integration-corev2:migrations'))`

func ApplyMigrations(ctx context.Context, pool *pgxpool.Pool) error {
	return applyMigrations(ctx, pool)
}

func applyMigrations(ctx context.Context, pool migrationPool) error {
	bootstrapTx, err := beginLockedMigration(ctx, pool, "bootstrap")
	if err != nil {
		return err
	}
	if _, err := bootstrapTx.Exec(ctx, `CREATE TABLE IF NOT EXISTS integration_schema_migrations (version TEXT PRIMARY KEY, applied_at TIMESTAMPTZ NOT NULL DEFAULT now())`); err != nil {
		_ = bootstrapTx.Rollback(ctx)
		return fmt.Errorf("create migrations table: %w", err)
	}
	if err := bootstrapTx.Commit(ctx); err != nil {
		return fmt.Errorf("commit migrations table: %w", err)
	}

	files, err := migrationFS.ReadDir("migrations")
	if err != nil {
		return fmt.Errorf("read migrations: %w", err)
	}
	versions := make([]string, 0, len(files))
	for _, file := range files {
		if file.IsDir() || filepath.Ext(file.Name()) != ".sql" {
			continue
		}
		versions = append(versions, file.Name())
	}
	sort.Strings(versions)

	for _, version := range versions {
		tx, err := beginLockedMigration(ctx, pool, version)
		if err != nil {
			return err
		}
		applied, err := alreadyApplied(ctx, tx, version)
		if err != nil {
			_ = tx.Rollback(ctx)
			return err
		}
		if applied {
			if err := tx.Commit(ctx); err != nil {
				return fmt.Errorf("commit migration %s: %w", version, err)
			}
			continue
		}
		sqlBytes, err := migrationFS.ReadFile("migrations/" + version)
		if err != nil {
			_ = tx.Rollback(ctx)
			return fmt.Errorf("read migration %s: %w", version, err)
		}
		if _, err := tx.Exec(ctx, string(sqlBytes)); err != nil {
			_ = tx.Rollback(ctx)
			return fmt.Errorf("apply migration %s: %w", version, err)
		}
		if _, err := tx.Exec(ctx, `INSERT INTO integration_schema_migrations (version) VALUES ($1)`, version); err != nil {
			_ = tx.Rollback(ctx)
			return fmt.Errorf("record migration %s: %w", version, err)
		}
		if err := tx.Commit(ctx); err != nil {
			return fmt.Errorf("commit migration %s: %w", version, err)
		}
	}
	return nil
}

func beginLockedMigration(ctx context.Context, pool migrationPool, version string) (pgx.Tx, error) {
	tx, err := pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin migration %s: %w", version, err)
	}
	if _, err := tx.Exec(ctx, migrationAdvisoryLockSQL); err != nil {
		_ = tx.Rollback(ctx)
		return nil, fmt.Errorf("lock migration %s: %w", version, err)
	}
	return tx, nil
}

func alreadyApplied(ctx context.Context, pool migrationQuerier, version string) (bool, error) {
	var exists bool
	if err := pool.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM integration_schema_migrations WHERE version = $1)`, version).Scan(&exists); err != nil {
		return false, fmt.Errorf("check migration %s: %w", version, err)
	}
	return exists, nil
}

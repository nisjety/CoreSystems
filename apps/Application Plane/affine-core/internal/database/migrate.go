package database

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

const (
	affineMigrationTableName = "affine_core_schema_migrations"
	createAffineMigrationTableSQL = `
CREATE TABLE IF NOT EXISTS affine_core_schema_migrations (
	name TEXT PRIMARY KEY,
	applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
)`
	affineMigrationExistsSQL = `
SELECT EXISTS(
	SELECT 1
	FROM affine_core_schema_migrations
	WHERE name = $1
)`
	recordAffineMigrationSQL = `
INSERT INTO affine_core_schema_migrations (name)
VALUES ($1)
ON CONFLICT (name) DO NOTHING`
)

type migrationExecutor interface {
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
	Begin(ctx context.Context) (migrationTx, error)
}

type migrationTx interface {
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
	Commit(ctx context.Context) error
	Rollback(ctx context.Context) error
}

type poolExecutor struct {
	pool *pgxpool.Pool
}

func (p poolExecutor) Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	return p.pool.Exec(ctx, sql, args...)
}

func (p poolExecutor) QueryRow(ctx context.Context, sql string, args ...any) pgx.Row {
	return p.pool.QueryRow(ctx, sql, args...)
}

func (p poolExecutor) Begin(ctx context.Context) (migrationTx, error) {
	return p.pool.Begin(ctx)
}

type migrationFile struct {
	name string
	sql  string
}

func RunMigrations(ctx context.Context, db *DB, dir string) error {
	if db == nil || db.Pool == nil {
		return fmt.Errorf("database pool is not configured")
	}

	return runMigrations(ctx, poolExecutor{pool: db.Pool}, dir)
}

func runMigrations(ctx context.Context, executor migrationExecutor, dir string) error {
	files, err := migrationFileNames(dir)
	if err != nil {
		return err
	}

	if _, err := executor.Exec(ctx, createAffineMigrationTableSQL); err != nil {
		return fmt.Errorf("ensure %s: %w", affineMigrationTableName, err)
	}

	for _, file := range files {
		var applied bool
		if err := executor.QueryRow(ctx, affineMigrationExistsSQL, file.name).Scan(&applied); err != nil {
			return fmt.Errorf("check migration %s: %w", file.name, err)
		}
		if applied {
			continue
		}

		if err := applyMigration(ctx, executor, file); err != nil {
			return err
		}
	}

	return nil
}

func applyMigration(ctx context.Context, executor migrationExecutor, file migrationFile) error {
	tx, err := executor.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin migration %s: %w", file.name, err)
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	if _, err := tx.Exec(ctx, file.sql); err != nil {
		return fmt.Errorf("%s: %w", file.name, err)
	}
	if _, err := tx.Exec(ctx, recordAffineMigrationSQL, file.name); err != nil {
		return fmt.Errorf("record migration %s: %w", file.name, err)
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit migration %s: %w", file.name, err)
	}
	committed = true

	return nil
}

func migrationFileNames(dir string) ([]migrationFile, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}

	files := make([]string, 0, len(entries))
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".up.sql") {
			continue
		}
		files = append(files, entry.Name())
	}
	sort.Strings(files)

	migrations := make([]migrationFile, 0, len(files))
	for _, name := range files {
		content, err := os.ReadFile(filepath.Join(dir, name))
		if err != nil {
			return nil, err
		}
		migrations = append(migrations, migrationFile{name: name, sql: string(content)})
	}

	return migrations, nil
}

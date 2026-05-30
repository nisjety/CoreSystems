package db

import (
	"context"
	"embed"
	"fmt"
	"io/fs"
	"regexp"
	"sort"
	"strconv"
	"strings"

	"github.com/jackc/pgx/v5/pgxpool"
)

//go:embed migrations/*.sql
var migrationsFS embed.FS

var migrationFilenameRE = regexp.MustCompile(`^(\d{4})_.+\.sql$`)

// ApplyMigrations applies every embedded migration that has not yet been
// recorded in schema_migrations. Each migration runs inside its own
// transaction so a failure leaves the database in a consistent state.
func ApplyMigrations(ctx context.Context, pool *pgxpool.Pool) error {
	if _, err := pool.Exec(ctx, schemaMigrationsDDL); err != nil {
		return fmt.Errorf("ensure schema_migrations: %w", err)
	}

	entries, err := loadEmbeddedMigrations()
	if err != nil {
		return err
	}

	applied, err := loadAppliedVersions(ctx, pool)
	if err != nil {
		return err
	}

	for _, m := range entries {
		if applied[m.version] {
			continue
		}
		if err := applyOne(ctx, pool, m); err != nil {
			return fmt.Errorf("migration %s: %w", m.name, err)
		}
	}
	return nil
}

const schemaMigrationsDDL = `
CREATE TABLE IF NOT EXISTS schema_migrations (
    version    INTEGER     PRIMARY KEY,
    name       TEXT        NOT NULL,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);`

type migration struct {
	version int
	name    string
	body    string
}

func loadEmbeddedMigrations() ([]migration, error) {
	var out []migration
	err := fs.WalkDir(migrationsFS, "migrations", func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		name := d.Name()
		match := migrationFilenameRE.FindStringSubmatch(name)
		if match == nil {
			return nil
		}
		version, err := strconv.Atoi(match[1])
		if err != nil {
			return fmt.Errorf("parse version from %q: %w", name, err)
		}
		body, err := fs.ReadFile(migrationsFS, path)
		if err != nil {
			return fmt.Errorf("read %q: %w", path, err)
		}
		out = append(out, migration{
			version: version,
			name:    strings.TrimSuffix(name, ".sql"),
			body:    string(body),
		})
		return nil
	})
	if err != nil {
		return nil, err
	}
	sort.Slice(out, func(i, j int) bool { return out[i].version < out[j].version })
	return out, nil
}

func loadAppliedVersions(ctx context.Context, pool *pgxpool.Pool) (map[int]bool, error) {
	rows, err := pool.Query(ctx, `SELECT version FROM schema_migrations`)
	if err != nil {
		return nil, fmt.Errorf("query schema_migrations: %w", err)
	}
	defer rows.Close()

	applied := make(map[int]bool)
	for rows.Next() {
		var v int
		if err := rows.Scan(&v); err != nil {
			return nil, fmt.Errorf("scan: %w", err)
		}
		applied[v] = true
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("rows: %w", err)
	}
	return applied, nil
}

func applyOne(ctx context.Context, pool *pgxpool.Pool, m migration) error {
	tx, err := pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, m.body); err != nil {
		return fmt.Errorf("exec body: %w", err)
	}
	if _, err := tx.Exec(ctx, `INSERT INTO schema_migrations (version, name) VALUES ($1, $2)`, m.version, m.name); err != nil {
		return fmt.Errorf("record version: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit: %w", err)
	}
	return nil
}

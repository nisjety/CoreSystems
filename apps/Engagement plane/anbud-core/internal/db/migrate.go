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

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

//go:embed all:migrations
var migrationsFS embed.FS

var migrationFilenameRE = regexp.MustCompile(`^(\d{4})_.+\.sql$`)

// migrationLockKey names the advisory lock that serialises migration runs. Any
// constant unique to this service will do; this one spells "anbudmig".
const migrationLockKey int64 = 0x616e6275646d6967

// ApplyMigrations applies every embedded migration not yet recorded in
// schema_migrations, in a single transaction.
//
// The transaction first takes a Postgres advisory lock, so replicas that start
// together queue up instead of racing: without it two of them can both try to
// create schema_migrations or apply the same version, and the loser dies on a
// unique violation. The lock is released at commit or rollback, so a crashed
// run cannot leave it held. Because everything is one transaction, a failing
// migration leaves the database exactly as it found it.
//
// After the migrations, in the same transaction, it states the application role's
// table privileges again (privileges.sql). That needs the role that owns the tables,
// so with the role model in use (docs/ROLES.md) the pool is the owner's: the
// application's role cannot even create schema_migrations.
func ApplyMigrations(ctx context.Context, pool *pgxpool.Pool) error {
	// Reject bad file names before touching the database.
	entries, err := loadEmbeddedMigrations()
	if err != nil {
		return err
	}

	tx, err := pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock($1)`, migrationLockKey); err != nil {
		return fmt.Errorf("take migration lock: %w", err)
	}
	if _, err := tx.Exec(ctx, schemaMigrationsDDL); err != nil {
		return fmt.Errorf("ensure schema_migrations: %w", err)
	}
	applied, err := loadAppliedVersions(ctx, tx)
	if err != nil {
		return err
	}

	for _, m := range entries {
		if applied[m.version] {
			continue
		}
		if err := applyOne(ctx, tx, m); err != nil {
			return fmt.Errorf("migration %s: %w", m.name, err)
		}
	}
	if _, err := tx.Exec(ctx, privilegesSQL); err != nil {
		return fmt.Errorf("apply role privileges: %w", err)
	}
	if _, err := tx.Exec(ctx, tenantPrivilegesSQL); err != nil {
		return fmt.Errorf("apply tenant role privileges: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit: %w", err)
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

// parseMigrationName extracts the version from "NNNN_description.sql".
func parseMigrationName(name string) (int, bool) {
	match := migrationFilenameRE.FindStringSubmatch(name)
	if match == nil {
		return 0, false
	}
	version, err := strconv.Atoi(match[1])
	if err != nil {
		return 0, false
	}
	return version, true
}

func loadEmbeddedMigrations() ([]migration, error) {
	return loadMigrations(migrationsFS, "migrations")
}

// loadMigrations reads the NNNN_description.sql files under root in version
// order. A .sql file whose name does not match is an error, not a skip: a typo
// such as 002_x.sql would otherwise never be applied while startup still logs
// "migrations applied". The same holds for a suffix that differs only in case,
// such as 0002_raw.SQL. Files that are not .sql (a README, .gitkeep) are ignored.
func loadMigrations(fsys fs.FS, root string) ([]migration, error) {
	var out []migration
	err := fs.WalkDir(fsys, root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		if !strings.HasSuffix(d.Name(), ".sql") {
			if hasSQLSuffixFold(d.Name()) {
				return fmt.Errorf("migration file %q must end in lower-case .sql", d.Name())
			}
			return nil
		}
		version, ok := parseMigrationName(d.Name())
		if !ok {
			return fmt.Errorf("migration file %q does not match NNNN_description.sql", d.Name())
		}
		body, err := fs.ReadFile(fsys, path)
		if err != nil {
			return fmt.Errorf("read %q: %w", path, err)
		}
		out = append(out, migration{
			version: version,
			name:    strings.TrimSuffix(d.Name(), ".sql"),
			body:    string(body),
		})
		return nil
	})
	if err != nil {
		return nil, err
	}
	sort.Slice(out, func(i, j int) bool { return out[i].version < out[j].version })
	for i := 1; i < len(out); i++ {
		if out[i-1].version == out[i].version {
			return nil, fmt.Errorf("duplicate migration version %04d (%s, %s)", out[i].version, out[i-1].name, out[i].name)
		}
	}
	return out, nil
}

// hasSQLSuffixFold reports whether name ends in ".sql" ignoring case.
func hasSQLSuffixFold(name string) bool {
	const suffix = ".sql"
	return len(name) >= len(suffix) && strings.EqualFold(name[len(name)-len(suffix):], suffix)
}

func loadAppliedVersions(ctx context.Context, tx pgx.Tx) (map[int]bool, error) {
	rows, err := tx.Query(ctx, `SELECT version FROM schema_migrations`)
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

// applyOne runs one migration and records its version inside the caller's
// transaction. A body with several statements is fine: pgx sends a query with no
// arguments over the simple protocol.
func applyOne(ctx context.Context, tx pgx.Tx, m migration) error {
	if _, err := tx.Exec(ctx, m.body); err != nil {
		return fmt.Errorf("exec body: %w", err)
	}
	if _, err := tx.Exec(ctx, `INSERT INTO schema_migrations (version, name) VALUES ($1, $2)`, m.version, m.name); err != nil {
		return fmt.Errorf("record version: %w", err)
	}
	return nil
}

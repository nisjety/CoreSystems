// Data Plane v2 schema migrator.
//
// Reads SQL files from infra/postgres/migrations/. Two filename conventions:
//
//	NNNNNNNNNNNN_description.sql        — forward migration ("up")
//	NNNNNNNNNNNN_description.down.sql   — paired rollback for the above version
//
// Tracks applied versions in schema_migrations. Usage:
//
//	migrator status                       # show applied + pending
//	migrator up                           # apply all pending forward
//	migrator up --to 20260601000000       # apply up to (and including) version
//	migrator down --to 20260601000000     # roll back DOWN to this version (exclusive)
//	migrator --migrations-dir <path>      # override default (./infra/postgres/migrations)
//
// Connection string from DATABASE_URL env var. Files ending in `.disabled` are skipped.
package main

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

const schemaTable = `
CREATE TABLE IF NOT EXISTS schema_migrations (
    version    TEXT        PRIMARY KEY,
    name       TEXT        NOT NULL,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    duration_ms INTEGER    NOT NULL DEFAULT 0
)`

var versionPattern = regexp.MustCompile(`^(\d{14})_(.+)\.sql$`)

type connectDatabase func(context.Context, string) (*pgx.Conn, error)

type migration struct {
	version string
	name    string
	path    string
}

func main() {
	if len(os.Args) < 2 {
		usage()
		os.Exit(2)
	}
	cmd := os.Args[1]

	dir := defaultMigrationsDir()
	var toVersion string
	for i := 2; i < len(os.Args); i++ {
		switch os.Args[i] {
		case "--migrations-dir":
			if i+1 >= len(os.Args) {
				fail("--migrations-dir requires a path")
			}
			dir = os.Args[i+1]
			i++
		case "--to":
			if i+1 >= len(os.Args) {
				fail("--to requires a version")
			}
			toVersion = os.Args[i+1]
			i++
		}
	}

	dbURL := os.Getenv("DATABASE_URL")
	if dbURL == "" {
		fail("DATABASE_URL must be set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Minute)
	defer cancel()

	conn, err := connectWithRetry(
		ctx,
		dbURL,
		30*time.Second,
		250*time.Millisecond,
		pgx.Connect,
	)
	if err != nil {
		fail("connect: %v", err)
	}
	defer conn.Close(ctx)

	if _, err := conn.Exec(ctx, schemaTable); err != nil {
		fail("create schema_migrations: %v", err)
	}

	migrations, err := loadMigrations(dir)
	if err != nil {
		fail("load migrations: %v", err)
	}

	applied, err := loadApplied(ctx, conn)
	if err != nil {
		fail("load applied: %v", err)
	}

	switch cmd {
	case "status":
		printStatus(migrations, applied)
	case "up":
		if err := applyPending(ctx, conn, migrations, applied, toVersion); err != nil {
			fail("apply: %v", err)
		}
	case "down":
		if toVersion == "" {
			fail("down requires --to VERSION (exclusive lower bound)")
		}
		if err := rollback(ctx, conn, migrations, applied, toVersion); err != nil {
			fail("rollback: %v", err)
		}
	default:
		usage()
		os.Exit(2)
	}
}

func connectWithRetry(
	parent context.Context,
	dbURL string,
	retryWindow time.Duration,
	initialDelay time.Duration,
	connect connectDatabase,
) (*pgx.Conn, error) {
	ctx, cancel := context.WithTimeout(parent, retryWindow)
	defer cancel()

	delay := initialDelay
	var lastErr error
	for {
		conn, err := connect(ctx, dbURL)
		if err == nil {
			return conn, nil
		}
		lastErr = err

		timer := time.NewTimer(delay)
		select {
		case <-ctx.Done():
			timer.Stop()
			return nil, fmt.Errorf("database did not become ready within %s: %w", retryWindow, lastErr)
		case <-timer.C:
		}
		if delay < 2*time.Second {
			delay *= 2
			if delay > 2*time.Second {
				delay = 2 * time.Second
			}
		}
	}
}

// rollback applies *.down.sql files for every migration STRICTLY GREATER THAN
// toVersion, in reverse order. A missing .down.sql sibling is a hard error —
// we refuse to leave a migration applied with no path back. Each rollback runs
// in a transaction and removes the schema_migrations row on success.
func rollback(
	ctx context.Context,
	conn *pgx.Conn,
	migrations []migration,
	applied map[string]bool,
	toVersion string,
) error {
	// Walk highest → lowest, only those above toVersion and currently applied.
	rolled := 0
	for i := len(migrations) - 1; i >= 0; i-- {
		m := migrations[i]
		if m.version <= toVersion {
			break
		}
		if !applied[m.version] {
			continue
		}

		downPath := strings.TrimSuffix(m.path, ".sql") + ".down.sql"
		body, err := os.ReadFile(downPath)
		if err != nil {
			return fmt.Errorf("missing rollback for %s: %w (expected %s)", m.version, err, downPath)
		}

		start := time.Now()
		tx, err := conn.Begin(ctx)
		if err != nil {
			return fmt.Errorf("begin rollback tx for %s: %w", m.version, err)
		}
		if _, err := tx.Exec(ctx, string(body)); err != nil {
			_ = tx.Rollback(ctx)
			return fmt.Errorf("exec rollback %s: %w", m.version, err)
		}
		if _, err := tx.Exec(ctx,
			"DELETE FROM schema_migrations WHERE version = $1", m.version,
		); err != nil {
			_ = tx.Rollback(ctx)
			return fmt.Errorf("delete applied row %s: %w", m.version, err)
		}
		if err := tx.Commit(ctx); err != nil {
			return fmt.Errorf("commit rollback %s: %w", m.version, err)
		}
		fmt.Printf("← %s  %s  (%dms)\n", m.version, m.name, int(time.Since(start).Milliseconds()))
		rolled++
	}
	if rolled == 0 {
		fmt.Println("Nothing to roll back.")
	} else {
		fmt.Printf("Rolled back %d migration(s).\n", rolled)
	}
	return nil
}

func defaultMigrationsDir() string {
	if cwd, err := os.Getwd(); err == nil {
		candidate := filepath.Join(cwd, "infra", "postgres", "migrations")
		if _, err := os.Stat(candidate); err == nil {
			return candidate
		}
	}
	return "infra/postgres/migrations"
}

func loadMigrations(dir string) ([]migration, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, fmt.Errorf("read dir %s: %w", dir, err)
	}

	var migrations []migration
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		name := e.Name()
		if strings.HasSuffix(name, ".disabled") {
			continue
		}
		// Skip `.down.sql` siblings — they are picked up by the rollback path
		// based on the forward file's name. Listing them here would cause the
		// version pattern to match a non-existent forward and double-apply.
		if strings.HasSuffix(name, ".down.sql") {
			continue
		}
		match := versionPattern.FindStringSubmatch(name)
		if match == nil {
			continue
		}
		migrations = append(migrations, migration{
			version: match[1],
			name:    match[2],
			path:    filepath.Join(dir, name),
		})
	}

	sort.Slice(migrations, func(i, j int) bool {
		return migrations[i].version < migrations[j].version
	})
	return migrations, nil
}

func loadApplied(ctx context.Context, conn *pgx.Conn) (map[string]bool, error) {
	rows, err := conn.Query(ctx, "SELECT version FROM schema_migrations")
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	applied := make(map[string]bool)
	for rows.Next() {
		var v string
		if err := rows.Scan(&v); err != nil {
			return nil, err
		}
		applied[v] = true
	}
	return applied, rows.Err()
}

func printStatus(migrations []migration, applied map[string]bool) {
	fmt.Println("Version          Name                                Status")
	fmt.Println("─────────────────────────────────────────────────────────────")
	for _, m := range migrations {
		status := "pending"
		if applied[m.version] {
			status = "applied"
		}
		fmt.Printf("%-16s %-35s %s\n", m.version, m.name, status)
	}
	if len(migrations) == 0 {
		fmt.Println("(no migration files)")
	}
}

func applyPending(ctx context.Context, conn *pgx.Conn, migrations []migration, applied map[string]bool, toVersion string) error {
	pending := 0
	for _, m := range migrations {
		if applied[m.version] {
			continue
		}
		if toVersion != "" && m.version > toVersion {
			break
		}
		if err := applyOne(ctx, conn, m); err != nil {
			return err
		}
		pending++
	}
	if pending == 0 {
		fmt.Println("Already up to date.")
	} else {
		fmt.Printf("Applied %d migration(s).\n", pending)
	}
	return nil
}

func applyOne(ctx context.Context, conn *pgx.Conn, m migration) error {
	body, err := os.ReadFile(m.path)
	if err != nil {
		return fmt.Errorf("read %s: %w", m.path, err)
	}

	start := time.Now()
	tx, err := conn.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin tx for %s: %w", m.version, err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, string(body)); err != nil {
		return fmt.Errorf("exec %s: %w", m.version, err)
	}
	duration := int(time.Since(start).Milliseconds())

	_, err = tx.Exec(ctx,
		"INSERT INTO schema_migrations (version, name, duration_ms) VALUES ($1, $2, $3)",
		m.version, m.name, duration,
	)
	if err != nil {
		return fmt.Errorf("record %s: %w", m.version, err)
	}

	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit %s: %w", m.version, err)
	}

	fmt.Printf("✓ %s  %s  (%dms)\n", m.version, m.name, duration)
	return nil
}

func usage() {
	fmt.Fprintln(os.Stderr, "usage: migrator <status|up> [--to VERSION] [--migrations-dir PATH]")
}

func fail(format string, args ...any) {
	fmt.Fprintf(os.Stderr, "migrator: "+format+"\n", args...)
	os.Exit(1)
}

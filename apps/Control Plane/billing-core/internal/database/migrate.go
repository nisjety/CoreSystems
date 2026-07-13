package database

import (
	"context"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

// RunMigrations applies pending *.up.sql files from migrationsDir in
// lexicographic order, tracking applied versions in a schema_migrations ledger
// (the proven Data Plane v2 migrator pattern, ported per Phase 6 B4). Each
// migration is recorded so it runs exactly once across restarts — replacing the
// previous behaviour of re-executing every idempotent DDL file on every boot.
//
// Each migration runs inside its own transaction: the file body plus the ledger
// insert either commit together or roll back together.
//
// Existing pre-ledger databases deliberately execute every idempotent migration
// once before recording it. Merely baselining every file on disk would allow a
// newly shipped migration to be marked applied without its schema ever being
// created (for example, the permanent organization tombstone table).
func RunMigrations(ctx context.Context, db *DB, migrationsDir string) error {
	if _, err := db.Pool.Exec(ctx, `
		CREATE TABLE IF NOT EXISTS schema_migrations (
			version     TEXT        PRIMARY KEY,
			name        TEXT        NOT NULL DEFAULT '',
			applied_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
			duration_ms INTEGER     NOT NULL DEFAULT 0
		)`); err != nil {
		return fmt.Errorf("create schema_migrations: %w", err)
	}

	entries, err := os.ReadDir(migrationsDir)
	if err != nil {
		return fmt.Errorf("read migrations dir: %w", err)
	}

	var files []string
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		if strings.HasSuffix(e.Name(), ".up.sql") {
			files = append(files, e.Name())
		}
	}
	sort.Strings(files)

	for _, name := range files {
		version := strings.TrimSuffix(name, ".up.sql")

		var applied bool
		if err := db.Pool.QueryRow(ctx,
			"SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = $1)", version,
		).Scan(&applied); err != nil {
			return fmt.Errorf("check migration %s: %w", version, err)
		}
		if applied {
			continue
		}

		if err := applyOne(ctx, db, version, filepath.Join(migrationsDir, name)); err != nil {
			return err
		}
	}

	return nil
}

// applyOne runs a single migration file and records it, atomically — the file
// body and the ledger insert share one transaction.
func applyOne(ctx context.Context, db *DB, version, path string) error {
	body, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("read migration %s: %w", version, err)
	}

	start := time.Now()
	tx, err := db.Pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin tx for %s: %w", version, err)
	}
	defer func() { _ = tx.Rollback(ctx) }() // no-op after a successful Commit

	if _, err := tx.Exec(ctx, string(body)); err != nil {
		return fmt.Errorf("execute migration %s: %w", version, err)
	}

	durationMs := int(time.Since(start).Milliseconds())
	if _, err := tx.Exec(ctx,
		"INSERT INTO schema_migrations (version, name, duration_ms) VALUES ($1, $2, $3)",
		version, version, durationMs,
	); err != nil {
		return fmt.Errorf("record migration %s: %w", version, err)
	}

	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit migration %s: %w", version, err)
	}

	log.Printf("✓ applied migration %s (%dms)", version, durationMs)
	return nil
}

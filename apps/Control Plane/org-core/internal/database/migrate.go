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
// Each migration runs inside its own transaction: every statement in the file
// plus the ledger insert either commit together or roll back together.
//
// Adopt-existing baseline: org-core ran for a long time *without* a ledger,
// re-executing all idempotent DDL on every boot, so live databases already
// carry the full current schema. On the first boot after this ledger is
// introduced (the table is brand new) but the database already has application
// tables, we record every on-disk migration as already-applied instead of
// re-running it. That makes the cut-over a no-op on existing data, while a
// genuinely fresh database still runs every migration from scratch.
func RunMigrations(ctx context.Context, db *DB, migrationsDir string) error {
	// Probe whether the ledger predates this boot — drives the baseline below.
	var ledgerExisted bool
	if err := db.Pool.QueryRow(ctx,
		"SELECT to_regclass('public.schema_migrations') IS NOT NULL",
	).Scan(&ledgerExisted); err != nil {
		return fmt.Errorf("probe schema_migrations: %w", err)
	}

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

	// Adopt-existing baseline (see doc comment): first ever boot of the ledger
	// against an already-populated database — record everything as applied.
	if !ledgerExisted {
		var hasAppTables bool
		if err := db.Pool.QueryRow(ctx, `
			SELECT EXISTS(
				SELECT 1 FROM information_schema.tables
				WHERE table_schema = 'public' AND table_name <> 'schema_migrations'
			)`).Scan(&hasAppTables); err != nil {
			return fmt.Errorf("probe existing schema: %w", err)
		}
		if hasAppTables {
			for _, name := range files {
				version := strings.TrimSuffix(name, ".up.sql")
				if _, err := db.Pool.Exec(ctx,
					"INSERT INTO schema_migrations (version, name) VALUES ($1, $2) ON CONFLICT (version) DO NOTHING",
					version, version,
				); err != nil {
					return fmt.Errorf("baseline-record %s: %w", version, err)
				}
			}
			log.Printf("schema_migrations: adopted existing schema, baselined %d migration(s)", len(files))
			return nil
		}
	}

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

// applyOne runs a single migration file and records it, atomically. Statements
// are split so schema changes from one are visible to the next within the file,
// and the whole batch plus the ledger insert share one transaction.
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

	for _, stmt := range splitStatements(string(body)) {
		if stmt == "" {
			continue
		}
		if _, err := tx.Exec(ctx, stmt); err != nil {
			return fmt.Errorf("execute migration %s: %w", version, err)
		}
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

// splitStatements splits a SQL script on semicolons while correctly handling
// single-quoted strings, dollar-quoted strings ($$ ... $$ or $tag$ ... $tag$),
// line comments (--) and block comments (/* */).
func splitStatements(sql string) []string {
	var stmts []string
	var buf strings.Builder
	i := 0
	runes := []rune(sql)
	n := len(runes)

	peek := func(offset int) rune {
		if i+offset < n {
			return runes[i+offset]
		}
		return 0
	}

	for i < n {
		ch := runes[i]

		// Line comment: skip until newline
		if ch == '-' && peek(1) == '-' {
			for i < n && runes[i] != '\n' {
				buf.WriteRune(runes[i])
				i++
			}
			continue
		}

		// Block comment: skip /* ... */
		if ch == '/' && peek(1) == '*' {
			buf.WriteRune(ch)
			buf.WriteRune(runes[i+1])
			i += 2
			for i < n {
				if runes[i] == '*' && i+1 < n && runes[i+1] == '/' {
					buf.WriteRune(runes[i])
					buf.WriteRune(runes[i+1])
					i += 2
					break
				}
				buf.WriteRune(runes[i])
				i++
			}
			continue
		}

		// Single-quoted string: copy until closing quote (handle '' escapes)
		if ch == '\'' {
			buf.WriteRune(ch)
			i++
			for i < n {
				c := runes[i]
				buf.WriteRune(c)
				i++
				if c == '\'' {
					if i < n && runes[i] == '\'' {
						// Escaped quote: ''
						buf.WriteRune(runes[i])
						i++
					} else {
						break
					}
				}
			}
			continue
		}

		// Dollar-quoted string: $tag$ ... $tag$ (tag may be empty → $$)
		if ch == '$' {
			// Collect the dollar-quote tag
			j := i + 1
			for j < n && runes[j] != '$' && runes[j] != '\n' {
				j++
			}
			if j < n && runes[j] == '$' {
				tag := string(runes[i : j+1]) // e.g. "$$" or "$body$"
				// Write the opening tag
				for k := i; k <= j; k++ {
					buf.WriteRune(runes[k])
				}
				i = j + 1
				// Scan for the closing tag
				for i < n {
					if runes[i] == '$' {
						end := i + len([]rune(tag))
						if end <= n && string(runes[i:end]) == tag {
							for k := i; k < end; k++ {
								buf.WriteRune(runes[k])
							}
							i = end
							break
						}
					}
					buf.WriteRune(runes[i])
					i++
				}
				continue
			}
		}

		// Semicolon terminates a statement
		if ch == ';' {
			stmt := strings.TrimSpace(buf.String())
			if stmt != "" {
				stmts = append(stmts, stmt)
			}
			buf.Reset()
			i++
			continue
		}

		buf.WriteRune(ch)
		i++
	}

	// Trailing statement without semicolon
	if stmt := strings.TrimSpace(buf.String()); stmt != "" {
		stmts = append(stmts, stmt)
	}

	return stmts
}


package http

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
	"github.com/jackc/pgx/v5/pgxpool"
)

// newBrowserConsentPostgresFixture builds a throwaway schema holding the
// tables the browser_consent store touches, applies migration 044 to it, and
// returns the pool plus a repository over it. Skips without TEST_DATABASE_URL.
func newBrowserConsentPostgresFixture(t *testing.T) (*pgxpool.Pool, *users.Repository) {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping PostgreSQL browser consent test")
	}
	ctx := context.Background()
	admin, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("open admin pool: %v", err)
	}
	t.Cleanup(admin.Close)

	schema := fmt.Sprintf("browser_consent_%d", time.Now().UnixNano())
	if !regexp.MustCompile(`^[a-z0-9_]+$`).MatchString(schema) {
		t.Fatalf("unsafe fixture schema %q", schema)
	}
	if _, err := admin.Exec(ctx, "CREATE SCHEMA "+schema); err != nil {
		t.Fatalf("create fixture schema: %v", err)
	}
	t.Cleanup(func() { _, _ = admin.Exec(context.Background(), "DROP SCHEMA "+schema+" CASCADE") })

	config, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		t.Fatalf("parse fixture DSN: %v", err)
	}
	config.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, config)
	if err != nil {
		t.Fatalf("open fixture pool: %v", err)
	}
	t.Cleanup(pool.Close)

	if _, err := pool.Exec(ctx, `
		CREATE TABLE users (id TEXT PRIMARY KEY);
		CREATE TABLE user_settings (
			id TEXT PRIMARY KEY,
			user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
			category VARCHAR(50) NOT NULL,
			settings JSONB NOT NULL DEFAULT '{}',
			created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
			updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
		);
		CREATE UNIQUE INDEX idx_user_settings_user_category ON user_settings(user_id, category);
		CREATE TABLE user_activity_log (
			id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
			user_id TEXT NOT NULL,
			action TEXT NOT NULL,
			resource TEXT NOT NULL DEFAULT '',
			details JSONB,
			ip_address TEXT NOT NULL DEFAULT '',
			user_agent TEXT NOT NULL DEFAULT '',
			created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
		);
		INSERT INTO users (id) VALUES ('user-a'), ('user-b');
	`); err != nil {
		t.Fatalf("create fixture tables: %v", err)
	}
	migration, err := os.ReadFile(filepath.Join("..", "..", "migrations", "044_user_settings_browser_consent_rls.up.sql"))
	if err != nil {
		t.Fatalf("read migration 044: %v", err)
	}
	if _, err := pool.Exec(ctx, string(migration)); err != nil {
		t.Fatalf("apply migration 044: %v", err)
	}
	return pool, users.NewRepository(&database.DB{Pool: pool})
}

func TestBrowserConsentPostgresScopedStoreAuditsAndErases(t *testing.T) {
	pool, repo := newBrowserConsentPostgresFixture(t)
	ctx := context.Background()

	decision, verr := validateBrowserConsentDecision(browserConsentDecisionRequest{
		StartHost: "example.org", Hosts: []string{"cdn.example.org"}, Decision: "allow", Scope: "always",
	}, time.Now())
	if verr != nil {
		t.Fatalf("validate: %v", verr)
	}
	saved, err := repo.MutateBrowserConsentSettings(ctx, "user-a", func(current map[string]any) (map[string]any, *users.BrowserConsentAudit, error) {
		doc, err := decodeBrowserConsentDocument(current)
		if err != nil {
			return nil, nil, err
		}
		next, replaced, err := upsertBrowserConsentDecision(doc, decision)
		if err != nil {
			return nil, nil, err
		}
		encoded, err := encodeBrowserConsentDocument(next)
		return encoded, browserConsentDecidedAudit(decision, replaced), err
	})
	if err != nil {
		t.Fatalf("mutate as user-a: %v", err)
	}
	doc, err := decodeBrowserConsentDocument(saved.Settings)
	if err != nil || len(doc.Origins) != 1 || doc.Origins[0].StartHost != "example.org" {
		t.Fatalf("saved document: %+v, %v", doc, err)
	}

	var audits int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM user_activity_log WHERE user_id = 'user-a' AND action = $1 AND resource = 'user_settings/browser_consent'`, browserConsentAuditDecided).Scan(&audits); err != nil || audits != 1 {
		t.Fatalf("audit rows: %d, %v", audits, err)
	}

	other, err := repo.GetBrowserConsentSettings(ctx, "user-b")
	if err != nil || other != nil {
		t.Fatalf("user-b must have no browser consent row: %+v, %v", other, err)
	}

	// RLS: only meaningful for a non-superuser connection (a superuser bypasses
	// row security even with FORCE).
	var superuser bool
	if err := pool.QueryRow(ctx, `SELECT rolsuper FROM pg_roles WHERE rolname = current_user`).Scan(&superuser); err != nil {
		t.Fatalf("read role: %v", err)
	}
	if !superuser {
		tx, err := pool.Begin(ctx)
		if err != nil {
			t.Fatalf("begin: %v", err)
		}
		if _, err := tx.Exec(ctx, `SELECT set_config('app.current_user_id', 'user-b', true)`); err != nil {
			t.Fatalf("scope to user-b: %v", err)
		}
		var visible int
		if err := tx.QueryRow(ctx, `SELECT count(*) FROM user_settings WHERE user_id = 'user-a'`).Scan(&visible); err != nil {
			t.Fatalf("scoped read: %v", err)
		}
		if visible != 0 {
			t.Fatalf("a user-b scoped transaction sees %d of user-a's rows", visible)
		}
		if _, err := tx.Exec(ctx, `INSERT INTO user_settings (id, user_id, category) VALUES ('forged', 'user-a', 'browser_consent_forged')`); err == nil {
			t.Fatal("a user-b scoped transaction must not write a user-a row")
		}
		_ = tx.Rollback(ctx)
	} else {
		t.Log("connection is superuser: RLS isolation assertions skipped")
	}

	// GDPR anonymize path: an unscoped delete by user_id removes the row.
	if _, err := pool.Exec(ctx, `DELETE FROM user_settings WHERE user_id = $1`, "user-a"); err != nil {
		t.Fatalf("erasure delete: %v", err)
	}
	if after, err := repo.GetBrowserConsentSettings(ctx, "user-a"); err != nil || after != nil {
		t.Fatalf("browser consent survived erasure: %+v, %v", after, err)
	}

	// GDPR hard-delete path: the users FK cascade removes the row.
	if _, err := repo.MutateBrowserConsentSettings(ctx, "user-b", func(map[string]any) (map[string]any, *users.BrowserConsentAudit, error) {
		encoded, err := encodeBrowserConsentDocument(browserConsentDocument{})
		return encoded, nil, err
	}); err != nil {
		t.Fatalf("seed user-b: %v", err)
	}
	if _, err := pool.Exec(ctx, `DELETE FROM users WHERE id = $1`, "user-b"); err != nil {
		t.Fatalf("hard delete: %v", err)
	}
	var remaining int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM user_settings WHERE user_id = 'user-b'`).Scan(&remaining); err != nil || remaining != 0 {
		t.Fatalf("browser consent survived the users cascade: %d, %v", remaining, err)
	}
}

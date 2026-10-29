package database

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

func TestProjectionGuardMigrationUsesCanonicalOrgScopeGUC(t *testing.T) {
	path := filepath.Join("..", "..", "migrations", "012_auth_projection_guards.up.sql")
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read projection guard migration: %v", err)
	}
	sql := string(contents)
	if strings.Contains(sql, "app.current_org_id") {
		t.Fatal("projection guard migration uses app.current_org_id; runtime sets app.current_org")
	}
	if count := strings.Count(sql, "current_setting('app.current_org', true)"); count != 6 {
		t.Fatalf("canonical app.current_org policy checks = %d; want 6", count)
	}
	if !strings.Contains(sql, "auth_organization_projection_versions") {
		t.Fatal("projection guard migration does not persist organization revisions")
	}
}

func TestProjectionGuardPoliciesEnforceTransactionOrgScope(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping Postgres RLS integration test")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer pool.Close()
	tx, err := pool.Begin(ctx)
	if err != nil {
		t.Fatalf("begin fixture transaction: %v", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	schema := "rls_projection_" + strings.ReplaceAll(time.Now().UTC().Format("150405.000000000"), ".", "")
	quotedSchema := pgx.Identifier{schema}.Sanitize()
	if _, err := tx.Exec(ctx, "CREATE SCHEMA "+quotedSchema); err != nil {
		t.Fatalf("create fixture schema: %v", err)
	}
	if _, err := tx.Exec(ctx, "GRANT USAGE ON SCHEMA "+quotedSchema+" TO org_core_app"); err != nil {
		t.Fatalf("grant fixture schema: %v", err)
	}
	if _, err := tx.Exec(ctx, "SET LOCAL search_path TO "+quotedSchema+", public"); err != nil {
		t.Fatalf("set fixture search path: %v", err)
	}

	path := filepath.Join("..", "..", "migrations", "012_auth_projection_guards.up.sql")
	migration, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	if _, err := tx.Exec(ctx, string(migration)); err != nil {
		t.Fatalf("apply projection migration in fixture: %v", err)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO auth_organization_tombstones (org_id) VALUES ('org-a'), ('org-b');
		INSERT INTO auth_organization_projection_versions (org_id, revision)
		VALUES ('org-a', 2), ('org-b', 1);
		INSERT INTO auth_membership_projection_versions (org_id, user_id, revision, desired_action)
		VALUES ('org-a', 'user-a', 1, 'upsert'), ('org-b', 'user-b', 1, 'upsert')`); err != nil {
		t.Fatalf("seed fixture rows: %v", err)
	}
	if _, err := tx.Exec(ctx, "SELECT set_config('app.current_org', 'org-a', true)"); err != nil {
		t.Fatalf("set canonical org scope: %v", err)
	}
	if _, err := tx.Exec(ctx, "SET LOCAL ROLE org_core_app"); err != nil {
		t.Fatalf("set RLS role: %v", err)
	}

	var tombstones int
	if err := tx.QueryRow(ctx, `SELECT COUNT(*) FROM auth_organization_tombstones`).Scan(&tombstones); err != nil {
		t.Fatalf("read scoped tombstones: %v", err)
	}
	if tombstones != 1 {
		t.Fatalf("scoped tombstones = %d; want 1", tombstones)
	}

	result, err := tx.Exec(ctx, `
		INSERT INTO auth_organization_projection_versions (org_id, revision)
		VALUES ('org-a', 1)
		ON CONFLICT (org_id) DO UPDATE SET revision = EXCLUDED.revision
		WHERE auth_organization_projection_versions.revision < EXCLUDED.revision`)
	if err != nil {
		t.Fatalf("apply delayed organization revision: %v", err)
	}
	if result.RowsAffected() != 0 {
		t.Fatal("delayed organization revision was accepted")
	}

	if _, err := tx.Exec(ctx, "SAVEPOINT cross_org_write"); err != nil {
		t.Fatalf("create savepoint: %v", err)
	}
	if _, err := tx.Exec(ctx, `INSERT INTO auth_organization_tombstones (org_id) VALUES ('org-c')`); err == nil {
		t.Fatal("cross-org tombstone insert succeeded; want RLS rejection")
	}
	if _, err := tx.Exec(ctx, "ROLLBACK TO SAVEPOINT cross_org_write"); err != nil {
		t.Fatalf("rollback rejected write: %v", err)
	}

	if _, err := tx.Exec(ctx, `
		INSERT INTO auth_membership_projection_versions (org_id, user_id, revision, desired_action)
		VALUES ('org-a', 'user-c', 2, 'remove')`); err != nil {
		t.Fatalf("same-org projection version insert rejected: %v", err)
	}
	if _, err := tx.Exec(ctx, "SELECT set_config('app.current_org', '', true)"); err != nil {
		t.Fatalf("clear org scope: %v", err)
	}
	if err := tx.QueryRow(ctx, `SELECT COUNT(*) FROM auth_organization_tombstones`).Scan(&tombstones); err != nil {
		t.Fatalf("read with empty scope: %v", err)
	}
	if tombstones != 0 {
		t.Fatalf("empty scope exposed %d tombstones; want 0", tombstones)
	}
}

func TestStrictRLSPoliciesFailClosedWithoutOrgScope(t *testing.T) {
	path := filepath.Join("..", "..", "migrations", "013_strict_rls_fail_closed.up.sql")
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read strict RLS migration: %v", err)
	}
	sql := string(contents)
	if strings.Contains(sql, "OR COALESCE(current_setting") {
		t.Fatal("strict RLS migration still permits an unset organization scope")
	}
	for _, table := range []string{"organizations", "organization_members", "organization_domains"} {
		if !strings.Contains(sql, table) {
			t.Fatalf("strict RLS migration does not cover %s", table)
		}
	}

	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping Postgres RLS integration test")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer pool.Close()
	tx, err := pool.Begin(ctx)
	if err != nil {
		t.Fatalf("begin fixture transaction: %v", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	schema := "rls_strict_" + strings.ReplaceAll(time.Now().UTC().Format("150405.000000000"), ".", "")
	quotedSchema := pgx.Identifier{schema}.Sanitize()
	if _, err := tx.Exec(ctx, "CREATE SCHEMA "+quotedSchema); err != nil {
		t.Fatalf("create fixture schema: %v", err)
	}
	if _, err := tx.Exec(ctx, "GRANT USAGE ON SCHEMA "+quotedSchema+" TO org_core_app"); err != nil {
		t.Fatalf("grant fixture schema: %v", err)
	}
	if _, err := tx.Exec(ctx, "CREATE TABLE "+quotedSchema+".organizations (id TEXT PRIMARY KEY)"); err != nil {
		t.Fatalf("create organizations fixture: %v", err)
	}
	if _, err := tx.Exec(ctx, "CREATE TABLE "+quotedSchema+".organization_domains (org_id TEXT NOT NULL, normalized_domain TEXT NOT NULL)"); err != nil {
		t.Fatalf("create domains fixture: %v", err)
	}
	if _, err := tx.Exec(ctx, "SET LOCAL search_path TO "+quotedSchema+", public"); err != nil {
		t.Fatalf("set fixture search path: %v", err)
	}
	if _, err := tx.Exec(ctx, sql); err != nil {
		t.Fatalf("apply strict RLS migration in fixture: %v", err)
	}
	if _, err := tx.Exec(ctx, "INSERT INTO organizations (id) VALUES ('org-a'), ('org-b')"); err != nil {
		t.Fatalf("seed organizations: %v", err)
	}
	if _, err := tx.Exec(ctx, "SET LOCAL ROLE org_core_app"); err != nil {
		t.Fatalf("set RLS role: %v", err)
	}

	var visible int
	if err := tx.QueryRow(ctx, "SELECT COUNT(*) FROM organizations").Scan(&visible); err != nil {
		t.Fatalf("query without org scope: %v", err)
	}
	if visible != 0 {
		t.Fatalf("empty scope exposed %d organizations; want 0", visible)
	}
	if _, err := tx.Exec(ctx, "SELECT set_config('app.current_org', 'org-a', true)"); err != nil {
		t.Fatalf("set canonical org scope: %v", err)
	}
	if err := tx.QueryRow(ctx, "SELECT COUNT(*) FROM organizations").Scan(&visible); err != nil {
		t.Fatalf("query with org scope: %v", err)
	}
	if visible != 1 {
		t.Fatalf("org-a scope exposed %d organizations; want 1", visible)
	}
}

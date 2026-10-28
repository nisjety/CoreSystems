//go:build integration

package registry

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
)

const migrationTestDatabaseURLEnv = "CAPABILITY_CORE_MIGRATION_TEST_DATABASE_URL"

func TestTenantScopeAndRiskMigrationQuarantinesMalformedLegacyScopes(t *testing.T) {
	dsn := strings.TrimSpace(os.Getenv(migrationTestDatabaseURLEnv))
	if dsn == "" {
		t.Skipf("set %s to an isolated PostgreSQL test database", migrationTestDatabaseURLEnv)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	conn, err := pgx.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to migration test database: %v", err)
	}
	t.Cleanup(func() { _ = conn.Close(context.Background()) })

	schemaName := "capability_migration_" + strings.ReplaceAll(uuid.NewString(), "-", "")
	schemaIdentifier := pgx.Identifier{schemaName}.Sanitize()
	if _, err := conn.Exec(ctx, "CREATE SCHEMA "+schemaIdentifier); err != nil {
		t.Fatalf("create isolated schema: %v", err)
	}
	t.Cleanup(func() {
		dropCtx, dropCancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer dropCancel()
		_, _ = conn.Exec(dropCtx, "DROP SCHEMA IF EXISTS "+schemaIdentifier+" CASCADE")
	})
	if _, err := conn.Exec(ctx, "SET search_path TO "+schemaIdentifier); err != nil {
		t.Fatalf("select isolated schema: %v", err)
	}

	applyMigrationFile(t, ctx, conn, "0003_capabilities_registry.up.sql")
	applyMigrationFile(t, ctx, conn, "0006_capability_availability_contract.up.sql")

	if _, err := conn.Exec(ctx, `
		INSERT INTO capabilities (id, org_id, kind, name)
		VALUES
			('cap.global', 'global', 'tool', 'Global tool'),
			('cap.tenant', 'tenant-a', 'tool', 'Tenant tool');

		INSERT INTO capability_scopes (id, capability_id, scope_kind, scope_value)
		VALUES
			('legacy-invalid-kind', 'cap.tenant', 'project', 'project-1'),
			('legacy-global-org-wildcard', 'cap.global', 'org', '*');
	`); err != nil {
		t.Fatalf("seed malformed legacy scopes: %v", err)
	}

	applyMigrationFile(t, ctx, conn, "0007_tenant_scopes_and_risk_constraints.up.sql")

	assertQuarantinedScope(t, ctx, conn, "legacy-invalid-kind", "tenant-a", "global", "project-1")
	assertQuarantinedScope(t, ctx, conn, "legacy-global-org-wildcard", "__quarantined_legacy__", "global", "*")

	applyMigrationFile(t, ctx, conn, "0007_tenant_scopes_and_risk_constraints.down.sql")
	var orgColumnExists bool
	if err := conn.QueryRow(ctx, `
		SELECT EXISTS (
			SELECT 1
			FROM information_schema.columns
			WHERE table_schema = current_schema()
			  AND table_name = 'capability_scopes'
			  AND column_name = 'org_id'
		)
	`).Scan(&orgColumnExists); err != nil {
		t.Fatalf("inspect rolled-back schema: %v", err)
	}
	if orgColumnExists {
		t.Fatal("down migration must remove capability_scopes.org_id")
	}
}

// 0018 seed, against a real database: cap.workspace.read is one low-risk row for the four
// read-only workspace tools, it starts unattested, and its tools are seeded read/none, so the
// ceiling the registry derives for it is read/none as well.
func TestWorkspaceReadCapabilityMigrationSeedsAnUnattestedReadRow(t *testing.T) {
	dsn := strings.TrimSpace(os.Getenv(migrationTestDatabaseURLEnv))
	if dsn == "" {
		t.Skipf("set %s to an isolated PostgreSQL test database", migrationTestDatabaseURLEnv)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	conn, err := pgx.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to migration test database: %v", err)
	}
	t.Cleanup(func() { _ = conn.Close(context.Background()) })

	schemaName := "capability_migration_" + strings.ReplaceAll(uuid.NewString(), "-", "")
	schemaIdentifier := pgx.Identifier{schemaName}.Sanitize()
	if _, err := conn.Exec(ctx, "CREATE SCHEMA "+schemaIdentifier); err != nil {
		t.Fatalf("create isolated schema: %v", err)
	}
	t.Cleanup(func() {
		dropCtx, dropCancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer dropCancel()
		_, _ = conn.Exec(dropCtx, "DROP SCHEMA IF EXISTS "+schemaIdentifier+" CASCADE")
	})
	if _, err := conn.Exec(ctx, "SET search_path TO "+schemaIdentifier); err != nil {
		t.Fatalf("select isolated schema: %v", err)
	}

	// Every migration up to and including 0018, in the order the boot applies them, so the row
	// is judged against the tables it lands in rather than against hand-built ones.
	paths, err := filepath.Glob(filepath.Join("..", "..", "migrations", "*.up.sql"))
	if err != nil || len(paths) == 0 {
		t.Fatalf("list migrations: %v", err)
	}
	for _, path := range paths {
		name := filepath.Base(path)
		if name > "0018_workspace_read_capability.up.sql" {
			break
		}
		applyMigrationFile(t, ctx, conn, name)
	}

	var riskLevel, availabilityState, reasonCode, sideEffectClass, approvalFloor string
	var healthNotChecked bool
	if err := conn.QueryRow(ctx, `
		SELECT risk_level, availability_state, availability_reason_code, side_effect_class,
		       approval_floor, health_checked_at IS NULL
		FROM capabilities
		WHERE id = 'cap.workspace.read' AND org_id = 'global'
	`).Scan(&riskLevel, &availabilityState, &reasonCode, &sideEffectClass, &approvalFloor, &healthNotChecked); err != nil {
		t.Fatalf("read cap.workspace.read: %v", err)
	}
	if riskLevel != "low" || availabilityState != "unavailable" || reasonCode != "health_not_attested" ||
		!healthNotChecked || sideEffectClass != "read" || approvalFloor != "none" {
		t.Fatalf("cap.workspace.read risk=%s state=%s reason=%s class=%s floor=%s unattested=%t; want low, unavailable, health_not_attested, read, none, unattested",
			riskLevel, availabilityState, reasonCode, sideEffectClass, approvalFloor, healthNotChecked)
	}

	var seeded int
	if err := conn.QueryRow(ctx, `
		SELECT count(*) FROM capability_tools
		WHERE org_id = 'global' AND capability_id = 'cap.workspace.read'
		  AND source = 'seed' AND side_effect_class = 'read' AND approval_floor = 'none'
	`).Scan(&seeded); err != nil {
		t.Fatalf("count seeded workspace tools: %v", err)
	}
	if seeded != 4 {
		t.Fatalf("seeded %d workspace tools, want the four read tools", seeded)
	}
}

func applyMigrationFile(t *testing.T, ctx context.Context, conn *pgx.Conn, name string) {
	t.Helper()
	contents, err := os.ReadFile(filepath.Join("..", "..", "migrations", name))
	if err != nil {
		t.Fatalf("read migration %s: %v", name, err)
	}
	if _, err := conn.Exec(ctx, string(contents)); err != nil {
		t.Fatalf("apply migration %s: %v", name, err)
	}
}

func assertQuarantinedScope(
	t *testing.T,
	ctx context.Context,
	conn *pgx.Conn,
	id string,
	wantOrgID string,
	wantScopeKind string,
	wantScopeValue string,
) {
	t.Helper()
	var orgID, scopeKind, scopeValue string
	var revoked bool
	err := conn.QueryRow(ctx, `
		SELECT org_id, scope_kind, scope_value, revoked_at IS NOT NULL
		FROM capability_scopes
		WHERE id = $1
	`, id).Scan(&orgID, &scopeKind, &scopeValue, &revoked)
	if err != nil {
		t.Fatalf("read quarantined scope %s: %v", id, err)
	}
	if !revoked {
		t.Fatalf("scope %s remains active", id)
	}
	if orgID != wantOrgID || scopeKind != wantScopeKind || scopeValue != wantScopeValue {
		t.Fatalf(
			"scope %s = org %q kind %q value %q, want org %q kind %q value %q",
			id,
			orgID,
			scopeKind,
			scopeValue,
			wantOrgID,
			wantScopeKind,
			wantScopeValue,
		)
	}
}

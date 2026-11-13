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

// isolatedMigrationConn opens the migration test database in a fresh schema that is dropped at
// the end of the test.
func isolatedMigrationConn(t *testing.T, ctx context.Context) *pgx.Conn {
	t.Helper()
	dsn := strings.TrimSpace(os.Getenv(migrationTestDatabaseURLEnv))
	if dsn == "" {
		t.Skipf("set %s to an isolated PostgreSQL test database", migrationTestDatabaseURLEnv)
	}
	conn, err := pgx.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to migration test database: %v", err)
	}
	t.Cleanup(func() { _ = conn.Close(context.Background()) })
	schemaIdentifier := pgx.Identifier{"capability_migration_" + strings.ReplaceAll(uuid.NewString(), "-", "")}.Sanitize()
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
	return conn
}

func applyEveryUpMigration(t *testing.T, ctx context.Context, conn *pgx.Conn) {
	t.Helper()
	paths, err := filepath.Glob(filepath.Join("..", "..", "migrations", "*.up.sql"))
	if err != nil || len(paths) == 0 {
		t.Fatalf("list migrations: %v", err)
	}
	for _, path := range paths {
		applyMigrationFile(t, ctx, conn, filepath.Base(path))
	}
}

// The entrypoint re-applies every *.up.sql at every boot, so a second full pass must succeed.
// This covers 0039-0042 (and every earlier file) against a real database.
func TestEveryUpMigrationAppliesTwice(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	conn := isolatedMigrationConn(t, ctx)
	applyEveryUpMigration(t, ctx, conn)
	applyEveryUpMigration(t, ctx, conn)
}

// W4-CC-7, against PostgreSQL: both custom REST capabilities start unavailable and unattested, and
// a later boot's re-apply does not reset an attestation execution-core made in between.
func TestCustomRestCapabilitiesStartUnavailableAndSurviveReapply(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	conn := isolatedMigrationConn(t, ctx)
	applyEveryUpMigration(t, ctx, conn)

	for _, id := range []string{"cap.custom_rest.read", "cap.custom_rest.write"} {
		var state, reason, mode string
		var unattested bool
		if err := conn.QueryRow(ctx, `
			SELECT availability_state, availability_reason_code, execution_mode, health_checked_at IS NULL
			FROM capabilities WHERE id = $1 AND org_id = 'global'
		`, id).Scan(&state, &reason, &mode, &unattested); err != nil {
			t.Fatalf("read %s: %v", id, err)
		}
		if state != "unavailable" || reason != "health_not_attested" || mode != "unavailable" || !unattested {
			t.Fatalf("%s state=%s reason=%s mode=%s unattested=%t; want unavailable, health_not_attested, unavailable, unattested",
				id, state, reason, mode, unattested)
		}
	}

	// execution-core attests the read capability; then the service reboots and re-applies 0041.
	if _, err := conn.Exec(ctx, `
		UPDATE capabilities
		SET availability_state = 'available', availability_reason_code = '', availability_reason = '',
		    execution_mode = 'agentic', health_checked_at = now()
		WHERE id = 'cap.custom_rest.read' AND org_id = 'global'
	`); err != nil {
		t.Fatalf("attest cap.custom_rest.read: %v", err)
	}
	applyMigrationFile(t, ctx, conn, "0041_custom_rest_capabilities.up.sql")
	var state string
	var attested bool
	if err := conn.QueryRow(ctx, `
		SELECT availability_state, health_checked_at IS NOT NULL
		FROM capabilities WHERE id = 'cap.custom_rest.read' AND org_id = 'global'
	`).Scan(&state, &attested); err != nil {
		t.Fatalf("read cap.custom_rest.read after re-apply: %v", err)
	}
	if state != "available" || !attested {
		t.Fatalf("re-applying 0041 reset the attestation: state=%s attested=%t", state, attested)
	}

	var floor, class string
	if err := conn.QueryRow(ctx, `
		SELECT side_effect_class, approval_floor FROM capability_tools
		WHERE org_id = 'global' AND capability_id = 'cap.custom_rest.write' AND tool_name = 'custom_rest.write'
	`).Scan(&class, &floor); err != nil {
		t.Fatalf("read custom_rest.write class: %v", err)
	}
	if class != "external_write" || floor != "ask" {
		t.Fatalf("custom_rest.write class=%s floor=%s; want external_write/ask", class, floor)
	}
}

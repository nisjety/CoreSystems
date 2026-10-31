package db

import (
	"context"
	"testing"

	"github.com/jackc/pgx/v5"
)

// The database tests live in package db_test, which may import dbtest. dbtest
// imports this package, so they cannot stay inside it. These exports give that
// external package the unexported runner it needs. Test builds only.

// SchemaMigrationsDDL is the DDL ApplyMigrations runs for schema_migrations.
var SchemaMigrationsDDL = schemaMigrationsDDL

// PrivilegesSQL is the script ApplyMigrations re-asserts after every run.
var PrivilegesSQL = privilegesSQL

// TenantPrivilegesSQL is the same for the schema tenant (migration 0007).
var TenantPrivilegesSQL = tenantPrivilegesSQL

// EmbeddedMigrationBodies returns the SQL of every embedded migration, in version order.
func EmbeddedMigrationBodies(t testing.TB) []string {
	t.Helper()
	entries, err := loadEmbeddedMigrations()
	if err != nil {
		t.Fatalf("loadEmbeddedMigrations: %v", err)
	}
	bodies := make([]string, 0, len(entries))
	for _, m := range entries {
		bodies = append(bodies, m.body)
	}
	return bodies
}

// EmbeddedMigration returns the name and the SQL of the embedded migration with this
// version, and fails t when there is none.
func EmbeddedMigration(t testing.TB, version int) (name, body string) {
	t.Helper()
	entries, err := loadEmbeddedMigrations()
	if err != nil {
		t.Fatalf("loadEmbeddedMigrations: %v", err)
	}
	for _, m := range entries {
		if m.version == version {
			return m.name, m.body
		}
	}
	t.Fatalf("the embedded migrations have no version %d", version)
	return "", ""
}

// ApplyOne runs one migration body and records its version inside tx, as
// ApplyMigrations does for each pending migration.
func ApplyOne(ctx context.Context, tx pgx.Tx, version int, name, body string) error {
	return applyOne(ctx, tx, migration{version: version, name: name, body: body})
}

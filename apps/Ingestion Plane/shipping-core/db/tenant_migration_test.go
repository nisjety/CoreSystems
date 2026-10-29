package db

import (
	"strings"
	"testing"
)

func TestTenantSecurityMigrationIsFailClosed(t *testing.T) {
	migration, err := migrationsFS.ReadFile("migrations/0004_tenant_security.up.sql")
	if err != nil {
		t.Fatalf("read tenant migration: %v", err)
	}
	sql := string(migration)
	for _, required := range []string{
		"org_id",
		"NOT NULL",
		"legacy-unscoped",
		"idempotency_key",
		"approval_id",
		"zdr",
		"retention_until",
		"UNIQUE INDEX",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("tenant migration missing %q", required)
		}
	}
	if !strings.Contains(sql, "org_id, idempotency_key") {
		t.Error("idempotency must be unique within the canonical organization")
	}
}

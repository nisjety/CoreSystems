package orgkeys

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

const migrationUp = "030_org_key_encryption_keys.up.sql"

func readMigration(t *testing.T, name string) string {
	t.Helper()
	body, err := os.ReadFile(filepath.Join("..", "..", "migrations", name))
	if err != nil {
		t.Fatalf("read %s: %v", name, err)
	}
	return string(body)
}

// The migration runner records each file once, but a pre-ledger database runs
// every file again (database.RunMigrations), so every statement must be safe to
// repeat. The Postgres test applies the file twice; this host test pins the shape.
func TestMigrationIsIdempotent(t *testing.T) {
	sql := readMigration(t, migrationUp)
	statements := map[*regexp.Regexp]*regexp.Regexp{
		regexp.MustCompile(`(?im)^\s*CREATE TABLE\s+`):           regexp.MustCompile(`(?im)^\s*CREATE TABLE IF NOT EXISTS\s+`),
		regexp.MustCompile(`(?im)^\s*CREATE (UNIQUE )?INDEX\s+`): regexp.MustCompile(`(?im)^\s*CREATE (UNIQUE )?INDEX IF NOT EXISTS\s+`),
	}
	for pattern, safe := range statements {
		if len(pattern.FindAllString(sql, -1)) != len(safe.FindAllString(sql, -1)) {
			t.Fatalf("every %q must be IF NOT EXISTS", pattern.String())
		}
	}
	policies := regexp.MustCompile(`(?im)^CREATE POLICY\s+(\S+)`).FindAllStringSubmatch(sql, -1)
	if len(policies) != 2 {
		t.Fatalf("policies = %d, want 2 (keys and outbox)", len(policies))
	}
	for _, policy := range policies {
		if !strings.Contains(sql, "DROP POLICY IF EXISTS "+policy[1]+" ") {
			t.Fatalf("policy %s is not dropped first", policy[1])
		}
	}
	if strings.Contains(strings.ToUpper(sql), "ALTER TABLE ORG_KEY_ENCRYPTION_KEYS ADD") {
		t.Fatal("an ADD COLUMN must be IF NOT EXISTS")
	}
}

func TestMigrationPinsCascadeShredAndLeastPrivilege(t *testing.T) {
	sql := readMigration(t, migrationUp)
	for _, required := range []string{
		"REFERENCES organizations(id) ON DELETE CASCADE",
		"ON org_key_encryption_keys (org_id) WHERE state = 'active'",
		"state = 'destroyed' AND wrapped_kek IS NULL",
		"octet_length(wrapped_kek) = 60",
		"CHECK (subject = '" + AuditSubject + "')",
		"REVOKE ALL ON org_key_encryption_keys FROM org_core_app;",
		"GRANT SELECT, INSERT, UPDATE ON org_key_encryption_keys TO org_core_app;",
		"REVOKE ALL ON org_key_master_canaries FROM org_core_app;",
		"GRANT SELECT, INSERT ON org_key_audit_outbox TO org_core_app;",
		"ALTER TABLE org_key_encryption_keys ENABLE ROW LEVEL SECURITY;",
		"ALTER TABLE org_key_audit_outbox ENABLE ROW LEVEL SECURITY;",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration is missing %q", required)
		}
	}
	if strings.Contains(sql, "DELETE ON org_key_encryption_keys") {
		t.Fatal("the runtime role must not delete KEK rows")
	}
	down := readMigration(t, "030_org_key_encryption_keys.down.sql")
	for _, table := range []string{"org_key_audit_outbox", "org_key_master_canaries", "org_key_encryption_keys"} {
		if !strings.Contains(down, "DROP TABLE IF EXISTS "+table+";") {
			t.Fatalf("down migration does not drop %s", table)
		}
	}
}

// Migration 030 follows 029 (028 and 029 are taken by identity domains and SCIM).
func TestMigrationNumberIsFree(t *testing.T) {
	entries, err := os.ReadDir(filepath.Join("..", "..", "migrations"))
	if err != nil {
		t.Fatalf("read migrations: %v", err)
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), "030_") && !strings.HasPrefix(entry.Name(), "030_org_key_encryption_keys.") {
			t.Fatalf("migration number 030 is shared with %s", entry.Name())
		}
	}
}

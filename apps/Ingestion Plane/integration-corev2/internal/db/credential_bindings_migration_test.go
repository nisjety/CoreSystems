package db

import (
	"regexp"
	"strings"
	"testing"
)

// W4-CC-3 (0015) and FU-18 (0016): text contracts for the two forward-only migrations this branch adds. The row-level
// behaviour needs a disposable Postgres; these need none.

func migrationStatement(t *testing.T, name string) string {
	t.Helper()
	raw, err := migrationFS.ReadFile("migrations/" + name)
	if err != nil {
		t.Fatalf("read migration %s: %v", name, err)
	}
	var statement strings.Builder
	for _, line := range strings.Split(string(raw), "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), "--") {
			continue
		}
		statement.WriteString(line)
		statement.WriteString("\n")
	}
	return regexp.MustCompile(`\s+`).ReplaceAllString(statement.String(), " ")
}

func TestCredentialBindingsMigration0015Contract(t *testing.T) {
	sql := migrationStatement(t, "0015_credential_bindings.sql")
	lower := strings.ToLower(sql)
	for _, required := range []string{
		"create table if not exists credential_bindings",
		"id text primary key check (id like 'cb\\_%')",
		"kind text not null check (kind in ('mcp_bearer', 'rest_api_key', 'rest_bearer'))",
		"header_name text not null check (header_name in (",
		"secret_ciphertext bytea not null",
		"key_version text not null",
		"status text not null check (status in ('active', 'revoked'))",
		// A revoked binding has a revocation time and an emptied ciphertext; an active one has a ciphertext.
		"check ((status = 'revoked') = (revoked_at is not null))",
		"check (status = 'revoked' or octet_length(secret_ciphertext) > 0)",
		"check (status = 'active' or octet_length(secret_ciphertext) = 0)",
		"where status = 'active'",
	} {
		if !strings.Contains(lower, required) {
			t.Errorf("migration 0015 missing %q in: %s", required, sql)
		}
	}
	// A plaintext column must never exist: the secret is sealed before it reaches the table.
	for _, forbidden := range []string{"secret text", "secret_plaintext", "plaintext", " drop ", "delete ", "truncate ", "alter table integration_connections"} {
		if strings.Contains(lower, forbidden) {
			t.Errorf("migration 0015 must not contain %q: %s", strings.TrimSpace(forbidden), sql)
		}
	}
	// The host CHECK refuses what the ADR section 4 grammar refuses.
	hostCheck := regexp.MustCompile(`host ~ '([^']+)'`).FindStringSubmatch(sql)
	if hostCheck == nil {
		t.Fatalf("migration 0015 has no host CHECK: %s", sql)
	}
	grammar := regexp.MustCompile(hostCheck[1])
	for _, host := range []string{"api.example.com", "a-b.example.co"} {
		if !grammar.MatchString(host) {
			t.Errorf("host CHECK refuses the valid host %q", host)
		}
	}
	for _, host := range []string{"API.example.com", "example", "*.example.com", "user@example.com", "example.com:443", "example.com.", "-a.example.com", "exa mple.com", "[::1]"} {
		if grammar.MatchString(host) {
			t.Errorf("host CHECK admits %q", host)
		}
	}
}

func TestWebhookEventConnectionMigration0016IsAdditiveAndNullable(t *testing.T) {
	sql := strings.ToLower(migrationStatement(t, "0016_webhook_event_connection_id.sql"))
	for _, required := range []string{
		"alter table integration_webhook_events",
		"add column if not exists connection_id text null",
		"check (connection_id is null or connection_id <> '')",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("migration 0016 missing %q in: %s", required, sql)
		}
	}
	// No default and no backfill: a row stored before 0016 stays bound to no connection.
	for _, forbidden := range []string{" default ", "update ", "not null", "delete ", "drop ", "truncate "} {
		if strings.Contains(sql, forbidden) {
			t.Errorf("migration 0016 must only add a nullable column, found %q: %s", strings.TrimSpace(forbidden), sql)
		}
	}
	if got := strings.Count(sql, "alter table"); got != 1 {
		t.Errorf("migration 0016 has %d ALTER TABLE statements, want 1", got)
	}
}

func TestTheBranchMigrationsFollowTheZDRMigrationInOrder(t *testing.T) {
	versions := migrationVersions(t)
	index := map[string]int{}
	for i, v := range versions {
		index[v] = i
	}
	order := []string{zdrMigration, "0014_scim_stub_payload_redaction.sql", "0015_credential_bindings.sql", "0016_webhook_event_connection_id.sql"}
	for i, name := range order {
		if _, ok := index[name]; !ok {
			t.Fatalf("migration %s is missing from %v", name, versions)
		}
		if i > 0 && index[name] <= index[order[i-1]] {
			t.Fatalf("migration %s applies before %s", name, order[i-1])
		}
	}
	if versions[len(versions)-1] != latestMigration {
		t.Fatalf("last migration = %s, latestMigration = %s", versions[len(versions)-1], latestMigration)
	}
}

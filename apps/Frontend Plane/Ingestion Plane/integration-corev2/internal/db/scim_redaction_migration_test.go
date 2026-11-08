package db

import (
	"regexp"
	"strings"
	"testing"
)

// WS05-S2: migration 0014 redacts the payloads the former SCIM stub stored.
// The behaviour on real rows is exercised by scim_redaction_postgres_test.go
// (needs a disposable database); this is the text contract that needs none.
func TestSCIMStubPayloadRedactionMigration0014Contract(t *testing.T) {
	raw, err := migrationFS.ReadFile("migrations/0014_scim_stub_payload_redaction.sql")
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	// Strip comments so the checks read the statement, not the explanation.
	var statement strings.Builder
	for _, line := range strings.Split(string(raw), "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), "--") {
			continue
		}
		statement.WriteString(strings.ToLower(line))
		statement.WriteString("\n")
	}
	sql := regexp.MustCompile(`\s+`).ReplaceAllString(statement.String(), " ")

	for _, required := range []string{
		"update integration_webhook_events",
		"set payload = '{}'::jsonb",
		"provider_key = 'scim'",
		// Idempotent: an already redacted row is not rewritten.
		"payload <> '{}'::jsonb",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("migration 0014 missing %q in: %s", required, sql)
		}
	}
	for _, forbidden := range []string{"delete ", "drop ", "truncate ", "alter table"} {
		if strings.Contains(sql, forbidden) {
			t.Errorf("migration 0014 must redact, not %q: %s", strings.TrimSpace(forbidden), sql)
		}
	}
	if got := strings.Count(sql, "update "); got != 1 {
		t.Errorf("migration 0014 has %d UPDATE statements, want exactly 1 scoped to provider_key = 'scim'", got)
	}
}

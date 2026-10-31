package database

import (
	"strings"
	"testing"
)

func TestAuditOutboxMigrationProvidesRetryAndTerminalVisibility(t *testing.T) {
	raw, err := migrationFS.ReadFile("migrations/003_audit_outbox.up.sql")
	if err != nil {
		t.Fatalf("read audit outbox migration: %v", err)
	}
	sql := string(raw)
	for _, required := range []string{
		"leads_audit_outbox", "event_id", "subject", "payload",
		"attempts", "next_attempt_at", "processing_at", "published_at", "terminal_at",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("audit outbox migration missing %q", required)
		}
	}
}

package db

import (
	"strings"
	"testing"
)

func TestAuditOutboxMigrationContract(t *testing.T) {
	raw, err := migrationFS.ReadFile("migrations/0010_audit_outbox.sql")
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	sql := strings.ToLower(string(raw))
	for _, required := range []string{
		"alter table integration_audit_events",
		"attempts integer",
		"next_attempt_at",
		"published_at",
		"terminal_at",
		"legacy_pre_outbox",
		"update integration_audit_events",
		"set published_at = now()",
		"create or replace function requeue_legacy_integration_audit_events",
		"requested_ids is null",
		"cardinality(requested_ids) between 1 and 1000",
		"legacy_pre_outbox is true",
		"for update skip locked",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("audit outbox migration missing %q", required)
		}
	}
	if strings.Contains(sql, "set published_at = null where legacy_pre_outbox") {
		t.Fatal("migration must not blindly enqueue every pre-outbox audit row")
	}
}

func TestAuditOutboxTerminalRecoveryMigrationContract(t *testing.T) {
	raw, err := migrationFS.ReadFile("migrations/0011_audit_outbox_terminal_recovery.sql")
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	sql := strings.ToLower(string(raw))
	for _, required := range []string{
		"requeue_terminal_integration_audit_events",
		"cardinality(requested_ids) between 1 and 100",
		"select distinct unnest(requested_ids)",
		"published_at is null",
		"terminal_at is not null",
		"attempts = 0",
		"revoke all",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("terminal recovery migration missing %q", required)
		}
	}
}

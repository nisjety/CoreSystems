package database

import (
	"os"
	"strings"
	"testing"
)

func TestUserAuditOutboxMigrationContract(t *testing.T) {
	content, err := os.ReadFile("../../migrations/014_gdpr_audit_outbox.up.sql")
	if err != nil {
		t.Fatalf("read audit outbox migration: %v", err)
	}
	sql := strings.ToLower(string(content))
	for _, required := range []string{
		"create table if not exists user_audit_outbox",
		"event_id text primary key",
		"for update skip locked",
		"velion.audit.v2.control.user-core.%",
		"terminal_at",
		"published_at",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("migration missing %q", required)
		}
	}
}

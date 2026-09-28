package database

import (
	"os"
	"strings"
	"testing"
)

func TestMailTaskAddressesMigrationContract(t *testing.T) {
	content, err := os.ReadFile("../../migrations/029_mail_task_addresses.up.sql")
	if err != nil {
		t.Fatalf("read mail task addresses migration: %v", err)
	}
	sql := strings.ToLower(string(content))
	for _, required := range []string{
		"create table if not exists mail_task_addresses",
		"alias",
		"text primary key",
		"org_id",
		"user_id",
		"revoked_at",
		"where revoked_at is null",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("migration missing %q", required)
		}
	}
}

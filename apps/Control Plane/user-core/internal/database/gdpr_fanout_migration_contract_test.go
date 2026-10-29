package database

import (
	"os"
	"strings"
	"testing"
)

func TestGDPRFanoutMigrationContract(t *testing.T) {
	content, err := os.ReadFile("../../migrations/016_gdpr_erasure_fanout.up.sql")
	if err != nil {
		t.Fatalf("read GDPR fanout migration: %v", err)
	}
	sql := strings.ToLower(string(content))
	for _, required := range []string{
		"create table if not exists user_erasure_fanout",
		"unique (operation_id, org_id)",
		"published_at",
		"terminal_at",
		"requeue_count",
		"fanout_snapshot_at",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("migration missing %q", required)
		}
	}
}

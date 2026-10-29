package repo

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const wikiOutboxMigration = "20260711180000_wiki_event_outbox.sql"

func TestWikiOutboxMigrationIsForwardSafeLeasedAndTenantBound(t *testing.T) {
	path := filepath.Join("..", "..", "..", "..", "infra", "postgres", "migrations", wikiOutboxMigration)
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read wiki outbox migration: %v", err)
	}
	sql := string(contents)
	for _, required := range []string{
		"CREATE TABLE IF NOT EXISTS wiki_event_outbox",
		"org_id TEXT NOT NULL",
		"event_type TEXT NOT NULL",
		"payload JSONB NOT NULL",
		"idempotency_key TEXT NOT NULL UNIQUE",
		"status IN ('pending', 'leased', 'delivered')",
		"lease_owner", "lease_until", "available_at", "delivered_at",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("migration missing %q", required)
		}
	}
	if strings.Contains(strings.ToUpper(sql), "DROP TABLE") {
		t.Fatal("forward migration must not drop historical tables")
	}

	downPath := filepath.Join("..", "..", "..", "..", "infra", "postgres", "migrations", "20260711180000_wiki_event_outbox.down.sql")
	down, err := os.ReadFile(downPath)
	if err != nil {
		t.Fatalf("read wiki outbox down migration: %v", err)
	}
	if !strings.Contains(string(down), "DROP TABLE IF EXISTS wiki_event_outbox") {
		t.Fatal("down migration does not remove only the new outbox")
	}
}

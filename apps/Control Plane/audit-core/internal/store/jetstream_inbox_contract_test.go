package store

import (
	"os"
	"strings"
	"testing"
)

func TestJetStreamInboxMigrationAndConflictHandlingArePresent(t *testing.T) {
	migration, err := os.ReadFile("migrations/002_jetstream_inbox.sql")
	if err != nil {
		t.Fatalf("read inbox migration: %v", err)
	}
	sql := string(migration)
	for _, required := range []string{
		"source_bus",
		"source_stream_sequence",
		"CREATE UNIQUE INDEX",
		"audit_events",
		"usage_events",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("inbox migration missing %q", required)
		}
	}

	storeSource, err := os.ReadFile("store.go")
	if err != nil {
		t.Fatalf("read store source: %v", err)
	}
	for _, required := range []string{
		"InsertAuditFromStream",
		"InsertUsageFromStream",
		"ON CONFLICT (source_bus, source_stream_sequence)",
		"pgx.ErrNoRows",
	} {
		if !strings.Contains(string(storeSource), required) {
			t.Errorf("idempotent store path missing %q", required)
		}
	}
}

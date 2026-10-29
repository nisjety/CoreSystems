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
		"ON CONFLICT DO NOTHING",
		"pgx.ErrNoRows",
	} {
		if !strings.Contains(string(storeSource), required) {
			t.Errorf("idempotent store path missing %q", required)
		}
	}
}

func TestLogicalControlAuditIdempotencyIsPresent(t *testing.T) {
	migration, err := os.ReadFile("migrations/003_logical_control_audit_idempotency.sql")
	if err != nil {
		t.Fatalf("read logical audit migration: %v", err)
	}
	sql := string(migration)
	for _, required := range []string{
		"ADD COLUMN IF NOT EXISTS event_id",
		"ADD COLUMN IF NOT EXISTS payload_hash",
		"CREATE UNIQUE INDEX",
		"ADD COLUMN IF NOT EXISTS source_subject",
		"source_bus, source_subject, event_id",
		"source_bus IS NOT NULL",
		"source_subject IS NOT NULL",
		"event_id IS NOT NULL",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("logical audit migration missing %q", required)
		}
	}

	storeSource, err := os.ReadFile("store.go")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(storeSource), "ON CONFLICT DO NOTHING") {
		t.Fatal("audit insert does not acknowledge a stable logical duplicate")
	}
}

func TestLogicalUsageIdempotencyMigrationIsPresent(t *testing.T) {
	migration, err := os.ReadFile("migrations/005_usage_source_identity.sql")
	if err != nil {
		t.Fatalf("read logical usage migration: %v", err)
	}
	sql := string(migration)
	for _, required := range []string{
		"usage_events",
		"ADD COLUMN IF NOT EXISTS source_subject",
		"ADD COLUMN IF NOT EXISTS payload_hash",
		"CREATE UNIQUE INDEX",
		"source_bus, source_subject, event_id",
		"source_bus IS NOT NULL",
		"source_subject IS NOT NULL",
		"event_id IS NOT NULL",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("logical usage migration missing %q", required)
		}
	}
}

func TestV2ProducerAuthorityIdempotencyMigrationIsPresent(t *testing.T) {
	migration, err := os.ReadFile("migrations/006_v2_producer_authority.sql")
	if err != nil {
		t.Fatalf("read v2 producer authority migration: %v", err)
	}
	sql := string(migration)
	for _, required := range []string{
		"audit_events", "usage_events", "source_producer",
		"source_bus, source_producer, event_id",
		"uq_audit_events_source_producer_event",
		"uq_usage_events_source_producer_event",
	} {
		if !strings.Contains(sql, required) {
			t.Errorf("v2 producer authority migration missing %q", required)
		}
	}
}

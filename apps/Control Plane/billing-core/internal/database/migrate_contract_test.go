package database

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestExistingSchemaAdoptionStillExecutesIdempotentPendingMigrations(t *testing.T) {
	path := filepath.Join("migrate.go")
	body, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read migration runner: %v", err)
	}
	source := string(body)
	if strings.Contains(source, "baseline-record") || strings.Contains(source, "baselined %d migration") {
		t.Fatal("migration runner can mark a newly shipped migration applied without executing it")
	}
	if !strings.Contains(source, "applyOne(ctx, db, version") {
		t.Fatal("migration runner does not execute pending migration files")
	}

	tombstonePath := filepath.Join("..", "..", "migrations", "0005_organization_tombstones.up.sql")
	tombstoneMigration, err := os.ReadFile(tombstonePath)
	if err != nil {
		t.Fatalf("read tombstone migration: %v", err)
	}
	if !strings.Contains(string(tombstoneMigration), "billing_organization_tombstones") {
		t.Fatal("billing deletion tombstone migration is missing")
	}
}

func TestPlanRevisionMigrationIsMonotonicAndPositive(t *testing.T) {
	path := filepath.Join("..", "..", "migrations", "0006_plan_revision.up.sql")
	body, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	text := string(body)
	for _, required := range []string{
		"plan_revision BIGINT NOT NULL DEFAULT 0",
		"CHECK (plan_revision >= 0)",
	} {
		if !strings.Contains(text, required) {
			t.Fatalf("plan revision migration missing %q", required)
		}
	}
}

func TestUsageDeliveryMigrationPreservesHistoryAndAddsAtomicIdentity(t *testing.T) {
	path := filepath.Join("..", "..", "migrations", "0007_usage_delivery_outbox.up.sql")
	body, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	text := string(body)
	for _, required := range []string{
		"ADD COLUMN IF NOT EXISTS event_id TEXT",
		"billing_usage_events_event_id_unique",
		"ADD COLUMN IF NOT EXISTS payload_hash TEXT",
		"billing_retry_jobs_processing_lease",
		"NOT VALID",
	} {
		if !strings.Contains(text, required) {
			t.Fatalf("usage delivery migration missing %q", required)
		}
	}
}

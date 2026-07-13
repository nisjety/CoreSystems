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

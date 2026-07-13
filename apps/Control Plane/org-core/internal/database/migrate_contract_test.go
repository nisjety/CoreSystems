package database

import (
	"os"
	"strings"
	"testing"
)

func TestExistingSchemaAdoptionCannotSkipNewMigrations(t *testing.T) {
	body, err := os.ReadFile("migrate.go")
	if err != nil {
		t.Fatalf("read migration runner: %v", err)
	}
	source := string(body)
	if strings.Contains(source, "baseline-record") || strings.Contains(source, "baselined %d migration") {
		t.Fatal("migration runner can mark newly shipped RLS/outbox migrations applied without executing them")
	}
	if !strings.Contains(source, "applyOne(ctx, db, version") {
		t.Fatal("migration runner does not execute pending migration files")
	}
}

package database

import (
	"os"
	"path/filepath"
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

func TestPlanChangeMigrationPinsPositiveRevisionAndTransactionalOutbox(t *testing.T) {
	path := filepath.Join("..", "..", "migrations", "014_plan_change_outbox.up.sql")
	body, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	text := string(body)
	for _, required := range []string{
		"plan_revision BIGINT NOT NULL DEFAULT 0",
		"organization_plan_change_outbox",
		"CHECK (revision > 0)",
		"app.current_org",
	} {
		if !strings.Contains(text, required) {
			t.Fatalf("plan change migration missing %q", required)
		}
	}
}

func TestProjectionConflictMigrationPersistsCanonicalSameRevisionState(t *testing.T) {
	path := filepath.Join("..", "..", "migrations", "015_auth_projection_conflict_state.up.sql")
	body, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	text := string(body)
	for _, required := range []string{
		"desired_name TEXT",
		"desired_owner_user_id TEXT",
		"desired_metadata JSONB",
		"desired_role TEXT",
		"'owner', 'admin', 'member', 'viewer'",
	} {
		if !strings.Contains(text, required) {
			t.Fatalf("projection conflict migration missing %q", required)
		}
	}
}

func TestDeletionRevisionMigrationPersistsCompletionAndExactRetryState(t *testing.T) {
	path := filepath.Join("..", "..", "migrations", "016_auth_deletion_revision.up.sql")
	body, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	text := string(body)
	for _, required := range []string{
		"revision BIGINT",
		"erasure_completed_at TIMESTAMPTZ",
		"deletion_receipt JSONB",
		"CHECK (revision > 0 AND revision <= 9007199254740991)",
		"REVOKE INSERT, UPDATE, DELETE ON auth_organization_tombstones FROM org_core_app",
		"GRANT INSERT (org_id, revision)",
		"GRANT UPDATE (erasure_completed_at, deletion_receipt)",
	} {
		if !strings.Contains(text, required) {
			t.Fatalf("deletion revision migration missing %q", required)
		}
	}
}

func TestGDPRAuditOutboxMigrationPinsDurabilityRetryAndDeadLetterState(t *testing.T) {
	path := filepath.Join("..", "..", "migrations", "017_gdpr_audit_outbox.up.sql")
	body, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	text := string(body)
	for _, required := range []string{
		"organization_gdpr_audit_outbox",
		"event_id TEXT PRIMARY KEY",
		"payload JSONB NOT NULL",
		"attempts INTEGER NOT NULL DEFAULT 0",
		"next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW()",
		"published_at TIMESTAMPTZ",
		"dead_lettered_at TIMESTAMPTZ",
		"last_error TEXT",
		"REVOKE INSERT, UPDATE, DELETE ON organization_gdpr_audit_outbox FROM org_core_app",
	} {
		if !strings.Contains(text, required) {
			t.Fatalf("GDPR audit outbox migration missing %q", required)
		}
	}
}

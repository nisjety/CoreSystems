package database

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

type fakeMigrationExecutor struct {
	tx *fakeMigrationTx
}

func (f *fakeMigrationExecutor) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	return pgconn.CommandTag{}, nil
}

func (f *fakeMigrationExecutor) QueryRow(context.Context, string, ...any) pgx.Row {
	return nil
}

func (f *fakeMigrationExecutor) Begin(context.Context) (migrationTx, error) {
	return f.tx, nil
}

type fakeMigrationTx struct {
	execCalls    int
	failAt       int
	committed    bool
	rolledBack   bool
	recordedName string
}

func (f *fakeMigrationTx) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	f.execCalls++
	if f.failAt == f.execCalls {
		return pgconn.CommandTag{}, errors.New("injected migration failure")
	}
	if strings.Contains(sql, "notification_core_schema_migrations") && len(args) == 1 {
		f.recordedName, _ = args[0].(string)
	}
	return pgconn.CommandTag{}, nil
}

func (f *fakeMigrationTx) Commit(context.Context) error {
	f.committed = true
	return nil
}

func (f *fakeMigrationTx) Rollback(context.Context) error {
	f.rolledBack = true
	return nil
}

func TestApplyMigrationCommitsSchemaAndRecordAtomically(t *testing.T) {
	tx := &fakeMigrationTx{}
	executor := &fakeMigrationExecutor{tx: tx}
	file := migrationFile{name: "007_test.up.sql", sql: "ALTER TABLE example ADD COLUMN tenant_id TEXT"}

	if err := applyMigration(context.Background(), executor, file); err != nil {
		t.Fatalf("applyMigration() error = %v", err)
	}
	if !tx.committed || tx.rolledBack {
		t.Fatalf("transaction state committed=%v rolledBack=%v", tx.committed, tx.rolledBack)
	}
	if tx.recordedName != file.name {
		t.Fatalf("recorded migration = %q, want %q", tx.recordedName, file.name)
	}
}

func TestApplyMigrationRollsBackWhenSchemaExecutionFails(t *testing.T) {
	tx := &fakeMigrationTx{failAt: 1}
	executor := &fakeMigrationExecutor{tx: tx}

	if err := applyMigration(context.Background(), executor, migrationFile{name: "007_test.up.sql", sql: "invalid"}); err == nil {
		t.Fatal("applyMigration() error = nil")
	}
	if tx.committed || !tx.rolledBack {
		t.Fatalf("transaction state committed=%v rolledBack=%v", tx.committed, tx.rolledBack)
	}
	if tx.recordedName != "" {
		t.Fatalf("failed migration was recorded as %q", tx.recordedName)
	}
}

func TestTenantScopeMigrationQuarantinesLegacyRowsAndDoesNotInventDelivery(t *testing.T) {
	migrations, err := migrationFileNames("../../migrations")
	if err != nil {
		t.Fatalf("migrationFileNames() error = %v", err)
	}
	var sql string
	for _, migration := range migrations {
		if migration.name == "007_tenant_scope_and_recipient_authority.up.sql" {
			sql = migration.sql
			break
		}
	}
	if sql == "" {
		t.Fatal("tenant scope migration not found")
	}
	required := []string{
		"notification_requests_org_idempotency_unique",
		"request_sha256",
		"notification_subscriber_memberships",
		"WHERE organization_id IS NULL",
		"ALTER COLUMN delivery_status SET DEFAULT 'submitted'",
		"ALTER COLUMN delivered_at DROP NOT NULL",
	}
	for _, fragment := range required {
		if !strings.Contains(sql, fragment) {
			t.Fatalf("migration missing %q", fragment)
		}
	}
	if strings.Contains(sql, "SET delivery_status = 'delivered'") {
		t.Fatal("migration invents delivered state for historical rows")
	}
}

func TestRetentionMigrationClassifiesExistingRowsAsStandardWithoutPersistingZDRContent(t *testing.T) {
	migrations, err := migrationFileNames("../../migrations")
	if err != nil {
		t.Fatalf("migrationFileNames() error = %v", err)
	}
	var sql string
	for _, migration := range migrations {
		if migration.name == "008_notification_retention_mode.up.sql" {
			sql = migration.sql
			break
		}
	}
	if sql == "" {
		t.Fatal("retention mode migration not found")
	}
	for _, fragment := range []string{
		"retention_mode TEXT NOT NULL DEFAULT 'standard'",
		"retention_mode IN ('standard', 'zdr')",
	} {
		if !strings.Contains(sql, fragment) {
			t.Fatalf("migration missing %q", fragment)
		}
	}
	if strings.Contains(sql, "UPDATE notification_requests SET retention_mode = 'zdr'") {
		t.Fatal("migration incorrectly reclassifies already persisted content as ZDR")
	}
}

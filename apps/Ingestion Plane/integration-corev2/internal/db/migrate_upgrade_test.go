package db

import (
	"context"
	"errors"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"

	"github.com/pashagolub/pgxmock/v4"
)

const authorizationRelationshipsMigration = "0009_action_receipt_authorization_relationships.sql"

func TestApplyMigrationsLocksBeforeCheckingAppliedVersion(t *testing.T) {
	pool, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock pool: %v", err)
	}
	expectMigrationsBootstrap(pool)
	for _, version := range migrationVersions(t) {
		expectMigrationState(pool, version, true)
		pool.ExpectCommit()
	}

	if err := applyMigrations(context.Background(), pool); err != nil {
		t.Fatalf("apply migrations: %v", err)
	}
	if err := pool.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet migration expectations: %v", err)
	}
}

func TestApplyMigrationsUpgradesDatabaseWithApplied0008(t *testing.T) {
	pool, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock pool: %v", err)
	}
	expectMigrationsBootstrap(pool)
	for _, version := range migrationVersions(t) {
		applied := version != authorizationRelationshipsMigration
		expectMigrationState(pool, version, applied)
		if applied {
			pool.ExpectCommit()
			continue
		}
		pool.ExpectExec("cannot strengthen provider-write authorization relationships").
			WillReturnResult(pgxmock.NewResult("ALTER TABLE", 0))
		pool.ExpectExec(regexp.QuoteMeta("INSERT INTO integration_schema_migrations (version) VALUES ($1)")).
			WithArgs(version).
			WillReturnResult(pgxmock.NewResult("INSERT", 1))
		pool.ExpectCommit()
	}

	if err := applyMigrations(context.Background(), pool); err != nil {
		t.Fatalf("apply migrations: %v", err)
	}
	if err := pool.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet migration expectations: %v", err)
	}
}

func TestApplyMigrationsFreshInstallEndsWithEnforced0009(t *testing.T) {
	pool, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock pool: %v", err)
	}
	versions := migrationVersions(t)
	if got := versions[len(versions)-1]; got != authorizationRelationshipsMigration {
		t.Fatalf("last migration = %q, want %q", got, authorizationRelationshipsMigration)
	}

	expectMigrationsBootstrap(pool)
	for _, version := range versions {
		expectMigrationState(pool, version, false)
		raw, err := migrationFS.ReadFile("migrations/" + version)
		if err != nil {
			t.Fatalf("read migration %s: %v", version, err)
		}
		pool.ExpectExec(regexp.QuoteMeta(string(raw))).WillReturnResult(pgxmock.NewResult("MIGRATION", 0))
		pool.ExpectExec(regexp.QuoteMeta("INSERT INTO integration_schema_migrations (version) VALUES ($1)")).
			WithArgs(version).
			WillReturnResult(pgxmock.NewResult("INSERT", 1))
		pool.ExpectCommit()
	}

	if err := applyMigrations(context.Background(), pool); err != nil {
		t.Fatalf("apply migrations: %v", err)
	}
	if err := pool.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet migration expectations: %v", err)
	}
}

func TestApplyMigrationsDoesNotRecordFailed0009Audit(t *testing.T) {
	pool, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock pool: %v", err)
	}
	auditErr := errors.New("legacy authorization relationship mismatch")
	expectMigrationsBootstrap(pool)
	for _, version := range migrationVersions(t) {
		applied := version != authorizationRelationshipsMigration
		expectMigrationState(pool, version, applied)
		if applied {
			pool.ExpectCommit()
			continue
		}
		pool.ExpectExec("cannot strengthen provider-write authorization relationships").WillReturnError(auditErr)
		pool.ExpectRollback()
	}

	err = applyMigrations(context.Background(), pool)
	if err == nil || !strings.Contains(err.Error(), authorizationRelationshipsMigration) || !errors.Is(err, auditErr) {
		t.Fatalf("apply migrations error = %v, want wrapped 0009 audit error", err)
	}
	if err := pool.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet migration expectations: %v", err)
	}
}

func TestApplyMigrationsReportsDatabaseFailures(t *testing.T) {
	testErr := errors.New("database failure")
	tests := []struct {
		name    string
		wantErr string
		setup   func(pgxmock.PgxPoolIface)
	}{
		{
			name:    "begin migrations table",
			wantErr: "begin migration bootstrap",
			setup: func(pool pgxmock.PgxPoolIface) {
				pool.ExpectBegin().WillReturnError(testErr)
			},
		},
		{
			name:    "lock migrations table",
			wantErr: "lock migration bootstrap",
			setup: func(pool pgxmock.PgxPoolIface) {
				pool.ExpectBegin()
				pool.ExpectExec(regexp.QuoteMeta(migrationAdvisoryLockSQL)).WillReturnError(testErr)
				pool.ExpectRollback()
			},
		},
		{
			name:    "create migrations table",
			wantErr: "create migrations table",
			setup: func(pool pgxmock.PgxPoolIface) {
				pool.ExpectBegin()
				pool.ExpectExec(regexp.QuoteMeta(migrationAdvisoryLockSQL)).WillReturnResult(pgxmock.NewResult("SELECT", 1))
				pool.ExpectExec(regexp.QuoteMeta("CREATE TABLE IF NOT EXISTS integration_schema_migrations")).WillReturnError(testErr)
				pool.ExpectRollback()
			},
		},
		{
			name:    "commit migrations table",
			wantErr: "commit migrations table",
			setup: func(pool pgxmock.PgxPoolIface) {
				pool.ExpectBegin()
				pool.ExpectExec(regexp.QuoteMeta(migrationAdvisoryLockSQL)).WillReturnResult(pgxmock.NewResult("SELECT", 1))
				pool.ExpectExec(regexp.QuoteMeta("CREATE TABLE IF NOT EXISTS integration_schema_migrations")).WillReturnResult(pgxmock.NewResult("CREATE TABLE", 0))
				pool.ExpectCommit().WillReturnError(testErr)
			},
		},
		{
			name:    "begin migration",
			wantErr: "begin migration 0001_init.sql",
			setup: func(pool pgxmock.PgxPoolIface) {
				expectMigrationsBootstrap(pool)
				pool.ExpectBegin().WillReturnError(testErr)
			},
		},
		{
			name:    "lock migration",
			wantErr: "lock migration 0001_init.sql",
			setup: func(pool pgxmock.PgxPoolIface) {
				expectMigrationsBootstrap(pool)
				pool.ExpectBegin()
				pool.ExpectExec(regexp.QuoteMeta(migrationAdvisoryLockSQL)).WillReturnError(testErr)
				pool.ExpectRollback()
			},
		},
		{
			name:    "check applied version",
			wantErr: "check migration 0001_init.sql",
			setup: func(pool pgxmock.PgxPoolIface) {
				expectMigrationsBootstrap(pool)
				pool.ExpectBegin()
				pool.ExpectExec(regexp.QuoteMeta(migrationAdvisoryLockSQL)).WillReturnResult(pgxmock.NewResult("SELECT", 1))
				pool.ExpectQuery(regexp.QuoteMeta("SELECT EXISTS (SELECT 1 FROM integration_schema_migrations WHERE version = $1)")).
					WithArgs("0001_init.sql").
					WillReturnError(testErr)
				pool.ExpectRollback()
			},
		},
		{
			name:    "record migration",
			wantErr: "record migration 0001_init.sql",
			setup: func(pool pgxmock.PgxPoolIface) {
				expectFirstMigrationPending(pool)
				pool.ExpectExec("CREATE TABLE IF NOT EXISTS integration_connections").WillReturnResult(pgxmock.NewResult("MIGRATION", 0))
				pool.ExpectExec(regexp.QuoteMeta("INSERT INTO integration_schema_migrations (version) VALUES ($1)")).
					WithArgs("0001_init.sql").
					WillReturnError(testErr)
				pool.ExpectRollback()
			},
		},
		{
			name:    "commit migration",
			wantErr: "commit migration 0001_init.sql",
			setup: func(pool pgxmock.PgxPoolIface) {
				expectFirstMigrationPending(pool)
				pool.ExpectExec("CREATE TABLE IF NOT EXISTS integration_connections").WillReturnResult(pgxmock.NewResult("MIGRATION", 0))
				pool.ExpectExec(regexp.QuoteMeta("INSERT INTO integration_schema_migrations (version) VALUES ($1)")).
					WithArgs("0001_init.sql").
					WillReturnResult(pgxmock.NewResult("INSERT", 1))
				pool.ExpectCommit().WillReturnError(testErr)
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			pool, err := pgxmock.NewPool()
			if err != nil {
				t.Fatalf("new pgx mock pool: %v", err)
			}
			tt.setup(pool)

			err = applyMigrations(context.Background(), pool)
			if err == nil || !strings.Contains(err.Error(), tt.wantErr) || !errors.Is(err, testErr) {
				t.Fatalf("apply migrations error = %v, want wrapped %q error", err, tt.wantErr)
			}
			if err := pool.ExpectationsWereMet(); err != nil {
				t.Fatalf("unmet migration expectations: %v", err)
			}
		})
	}
}

func expectMigrationsBootstrap(pool pgxmock.PgxPoolIface) {
	pool.ExpectBegin()
	pool.ExpectExec(regexp.QuoteMeta(migrationAdvisoryLockSQL)).WillReturnResult(pgxmock.NewResult("SELECT", 1))
	pool.ExpectExec(regexp.QuoteMeta("CREATE TABLE IF NOT EXISTS integration_schema_migrations")).
		WillReturnResult(pgxmock.NewResult("CREATE TABLE", 0))
	pool.ExpectCommit()
}

func expectMigrationState(pool pgxmock.PgxPoolIface, version string, applied bool) {
	pool.ExpectBegin()
	pool.ExpectExec(regexp.QuoteMeta(migrationAdvisoryLockSQL)).WillReturnResult(pgxmock.NewResult("SELECT", 1))
	pool.ExpectQuery(regexp.QuoteMeta("SELECT EXISTS (SELECT 1 FROM integration_schema_migrations WHERE version = $1)")).
		WithArgs(version).
		WillReturnRows(pgxmock.NewRows([]string{"exists"}).AddRow(applied))
}

func expectFirstMigrationPending(pool pgxmock.PgxPoolIface) {
	expectMigrationsBootstrap(pool)
	expectMigrationState(pool, "0001_init.sql", false)
}

func migrationVersions(t *testing.T) []string {
	t.Helper()
	entries, err := migrationFS.ReadDir("migrations")
	if err != nil {
		t.Fatalf("read migrations: %v", err)
	}
	versions := make([]string, 0, len(entries))
	for _, entry := range entries {
		if !entry.IsDir() && filepath.Ext(entry.Name()) == ".sql" {
			versions = append(versions, entry.Name())
		}
	}
	sort.Strings(versions)
	return versions
}

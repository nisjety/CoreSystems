package db

import (
	"context"
	"regexp"
	"strings"
	"testing"

	"github.com/pashagolub/pgxmock/v4"
)

const zdrMigration = "0013_model_connection_zdr.sql"

func readMigration(t *testing.T, name string) string {
	t.Helper()
	raw, err := migrationFS.ReadFile("migrations/" + name)
	if err != nil {
		t.Fatalf("read migration %s: %v", name, err)
	}
	return string(raw)
}

// stripSQLComments removes "-- ..." comments so the checks read statements, not prose.
func stripSQLComments(sql string) string {
	var out strings.Builder
	for _, line := range strings.Split(sql, "\n") {
		if i := strings.Index(line, "--"); i >= 0 {
			line = line[:i]
		}
		out.WriteString(line)
		out.WriteString("\n")
	}
	return out.String()
}

// createTableBlock returns the text of one CREATE TABLE statement.
func createTableBlock(t *testing.T, sql, table string) string {
	t.Helper()
	start := strings.Index(sql, "CREATE TABLE IF NOT EXISTS "+table+" (")
	if start < 0 {
		t.Fatalf("migration does not create %s", table)
	}
	end := strings.Index(sql[start:], "\n);")
	if end < 0 {
		t.Fatalf("cannot find the end of CREATE TABLE %s", table)
	}
	return sql[start : start+end+3]
}

func TestModelConnectionZDRMigrationContract(t *testing.T) {
	sql := readMigration(t, zdrMigration)
	lower := strings.ToLower(sql)

	side := strings.ToLower(stripSQLComments(createTableBlock(t, sql, "integration_connection_zdr")))
	ledger := strings.ToLower(stripSQLComments(createTableBlock(t, sql, "integration_connection_zdr_acknowledgements")))

	for _, required := range []string{
		"connection_id text primary key references integration_connections(id) on delete cascade",
		"capability in ('supported', 'not_supported', 'cannot_verify')",
		"'verevon_attested_deployment'", "'provider_terms_known'", "'provider_by_contract'", "'customer_asserted'", "'not_assessed'",
		"catalog_version integer not null check (catalog_version >= 1)",
		"digest text not null check (digest ~ '^[0-9a-f]{64}$')",
		"checked_at timestamptz not null",
	} {
		if !strings.Contains(side, required) {
			t.Errorf("side table integration_connection_zdr is missing %q", required)
		}
	}

	for _, required := range []string{
		"id text primary key",
		"connection_id text not null,",
		"capability in ('not_supported', 'cannot_verify')",
		"digest text not null check (digest ~ '^[0-9a-f]{64}$')",
		"text_sha256 text not null check (text_sha256 ~ '^[0-9a-f]{64}$')",
		"text_snapshot text not null check (text_snapshot <> '')",
		"locale text not null check (locale in ('no', 'en'))",
		"acknowledged_via text not null check (acknowledged_via in ('connect', 'first_use', 'settings'))",
		"org_zdr_at_ack boolean not null",
		"revoked_at timestamptz",
		"'capability_changed'", "'connection_removed'", "'owner_revoked'", "'text_version_superseded'",
		"check ((revoked_at is null) = (revoke_reason = ''))",
	} {
		if !strings.Contains(ledger, required) {
			t.Errorf("ledger integration_connection_zdr_acknowledgements is missing %q", required)
		}
	}
	// The ledger must not depend on the connection row: connections are soft-deleted and a
	// compliance record kept for a year may not cascade away with, or be blocked by, any row.
	if strings.Contains(ledger, "references") || strings.Contains(ledger, "cascade") || strings.Contains(ledger, "foreign key") {
		t.Fatal("the acknowledgement ledger has a foreign key; it must not depend on integration_connections")
	}

	for _, required := range []string{
		"create unique index if not exists integration_connection_zdr_ack_active_idx",
		"on integration_connection_zdr_acknowledgements (connection_id, digest)",
		"where revoked_at is null",
		"create or replace function integration_connection_zdr_ack_guard()",
		"drop trigger if exists integration_connection_zdr_ack_guard on integration_connection_zdr_acknowledgements",
		"before update on integration_connection_zdr_acknowledgements",
		"execute function integration_connection_zdr_ack_guard()",
		"only the revocation columns of a zdr acknowledgement may change",
		"a revoked zdr acknowledgement is final",
	} {
		if !strings.Contains(lower, required) {
			t.Errorf("migration is missing %q", required)
		}
	}
}

func TestModelConnectionZDRMigrationIsForwardOnlyAndTouchesNoExistingTable(t *testing.T) {
	sql := readMigration(t, zdrMigration)
	lower := strings.ToLower(sql)
	// Statements only: the explanatory comments legitimately name integration_connections.
	code := stripSQLComments(lower)
	for _, forbidden := range []string{
		"alter table integration_connections",
		"update integration_connections",
		"insert into integration_connections",
		"drop table",
		"truncate",
		"delete from",
	} {
		if strings.Contains(code, forbidden) {
			t.Errorf("migration contains %q: it must only add the two ZDR tables, their indexes and the guard", forbidden)
		}
	}
	// The backfill of existing connections is the service's idempotent reconcile, which computes
	// every digest with the one serialiser; the migration holds no copy of the capability rules
	// and no digest literal.
	if strings.Contains(code, "insert into integration_connection_zdr") {
		t.Error("migration backfills rows; the digest must be computed by package zdr, not duplicated in SQL")
	}
	if regexp.MustCompile(`[0-9a-f]{64}`).MatchString(code) {
		t.Error("migration contains a 64-hex literal; no digest is hard-coded in SQL")
	}
	if strings.Contains(code, "openai-codex") || strings.Contains(code, "codex_chatgpt") {
		t.Error("migration names a provider; capability rules live in the shared contract, not in SQL")
	}
}

func TestModelConnectionZDRMigrationIsReapplicable(t *testing.T) {
	code := strings.ToLower(readMigration(t, zdrMigration))
	create := regexp.MustCompile(`create (unique )?(table|index)( if not exists)? `)
	for _, match := range create.FindAllString(code, -1) {
		if !strings.Contains(match, "if not exists") {
			t.Errorf("%q is not guarded by IF NOT EXISTS", match)
		}
	}
	if strings.Contains(code, "create function") {
		t.Error("create function without OR REPLACE")
	}
	if strings.Count(code, "create trigger") != strings.Count(code, "drop trigger if exists") {
		t.Error("every CREATE TRIGGER needs a DROP TRIGGER IF EXISTS before it")
	}
}

// A 0012 database upgrades by applying 0013 alone, once, and a second run is a no-op.
func TestApplyMigrationsUpgradesA0012DatabaseWithOnly0013Pending(t *testing.T) {
	pool, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock pool: %v", err)
	}
	expectMigrationsBootstrap(pool)
	for _, version := range migrationVersions(t) {
		applied := version < zdrMigration
		expectMigrationState(pool, version, applied)
		if applied {
			pool.ExpectCommit()
			continue
		}
		pool.ExpectExec(regexp.QuoteMeta(readMigration(t, version))).WillReturnResult(pgxmock.NewResult("MIGRATION", 0))
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

	// Second start: everything, 0013 included, is recorded, and nothing is executed.
	again, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock pool: %v", err)
	}
	expectMigrationsBootstrap(again)
	for _, version := range migrationVersions(t) {
		expectMigrationState(again, version, true)
		again.ExpectCommit()
	}
	if err := applyMigrations(context.Background(), again); err != nil {
		t.Fatalf("re-apply migrations: %v", err)
	}
	if err := again.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet re-apply expectations: %v", err)
	}
}

func TestZDRMigrationIsTheLatestAndNumberedAfter0012(t *testing.T) {
	versions := migrationVersions(t)
	var seen012, seen013 bool
	for _, v := range versions {
		if v == "0012_email_sync_history_backfill.sql" {
			seen012 = true
		}
		if v == zdrMigration {
			seen013 = true
		}
	}
	if !seen012 || !seen013 {
		t.Fatalf("migration set = %v, want 0012 and 0013 present", versions)
	}
	if latestMigration != zdrMigration {
		t.Fatalf("latestMigration = %q, want %q", latestMigration, zdrMigration)
	}
}

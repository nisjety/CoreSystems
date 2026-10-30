package rawstore

import (
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"
)

// scalarText returns the single text a query yields.
func scalarText(t *testing.T, pool *pgxpool.Pool, query string, args ...any) string {
	t.Helper()
	var s string
	if err := pool.QueryRow(testCtx(t), query, args...).Scan(&s); err != nil {
		t.Fatalf("text query %q: %v", query, err)
	}
	return s
}

// T48: the migrated database has the five raw store tables, the ten triggers (the
// six append-only guards of migration 0002, the run provenance guard of migration
// 0003 and the three evidence run guards of migration 0005), and versions 1 to 7
// recorded once each. What migration 0006 adds to the tables and triggers has its own
// test, TestMigration0006Shape, and migration 0007 is in another schema (internal/docstore).
func TestMigration0002Shape(t *testing.T) {
	_, pool := openStore(t)

	tables := []string{"sync_runs", "raw_search_pages", "raw_documents", "download_attempts", "sync_days"}
	if got := count(t, pool, `
		SELECT count(*) FROM pg_class c
		JOIN pg_namespace n ON n.oid = c.relnamespace
		WHERE n.nspname = current_schema() AND c.relkind = 'r' AND c.relname::text = ANY($1::text[])`,
		tables); got != len(tables) {
		t.Fatalf("raw store tables = %d, want %d", got, len(tables))
	}

	triggers := []string{
		"raw_search_pages_append_only", "raw_search_pages_no_truncate",
		"raw_documents_append_only", "raw_documents_no_truncate",
		"download_attempts_append_only", "download_attempts_no_truncate",
		"sync_runs_provenance",
		"raw_search_pages_run_guard", "raw_documents_run_guard", "download_attempts_run_guard",
	}
	if got := count(t, pool, `
		SELECT count(*) FROM pg_trigger
		WHERE NOT tgisinternal AND tgname::text = ANY($1::text[])`,
		triggers); got != len(triggers) {
		t.Fatalf("evidence, provenance and run guard triggers = %d, want %d", got, len(triggers))
	}

	versions := []struct {
		version int
		name    string
	}{
		{1, "0001_baseline"}, {2, "0002_raw_store"}, {3, "0003_sync_guards"},
		{4, "0004_rejected_terminal"}, {5, "0005_evidence_run_guard"}, {6, "0006_redaction"},
		{7, "0007_tenant_documents"}, {8, "0008_ted_source"}, {9, "0009_workspace"}, {10, "0010_workspace_seen"}, {11, "0011_boamp_source"}, {12, "0012_fts_source"}, {13, "0013_bzp_source"}, {14, "0014_catalogue_projection"}, {15, "0015_catalogue_source_generation"}, {16, "0016_catalogue_source_guard"}, {17, "0017_matching_projection"},
	}
	for _, v := range versions {
		if got := count(t, pool, `SELECT count(*) FROM schema_migrations WHERE version = $1 AND name = $2`, v.version, v.name); got != 1 {
			t.Fatalf("version %d (%s) recorded %d times, want 1", v.version, v.name, got)
		}
	}
	if got := count(t, pool, `SELECT count(*) FROM schema_migrations`); got != len(versions) {
		t.Fatalf("schema_migrations holds %d versions, want exactly %d", got, len(versions))
	}
}

// R5: migration 0004 adds sync_days.rejected as a non-negative integer that defaults
// to 0, and replaces the done and capped constraints in place, under their old
// names, so that both name the new term.
func TestMigration0004Shape(t *testing.T) {
	_, pool := openStore(t)

	if got := count(t, pool, `
		SELECT count(*) FROM information_schema.columns
		WHERE table_schema = current_schema() AND table_name = 'sync_days' AND column_name = 'rejected'
		  AND data_type = 'integer' AND is_nullable = 'NO' AND column_default = '0'`); got != 1 {
		t.Fatalf("sync_days.rejected is not an INTEGER NOT NULL DEFAULT 0 column (matches %d)", got)
	}
	for _, name := range []string{"sync_days_done_adds_up", "sync_days_capped_is_saturated"} {
		if got := count(t, pool, `
			SELECT count(*) FROM pg_constraint
			WHERE conrelid = 'sync_days'::regclass AND contype = 'c' AND conname = $1
			  AND pg_get_constraintdef(oid) LIKE '%rejected%'`, name); got != 1 {
			t.Errorf("constraint %s: %d definition(s) name rejected, want exactly 1 constraint that does", name, got)
		}
	}
	if got := count(t, pool, `
		SELECT count(*) FROM pg_constraint
		WHERE conrelid = 'sync_days'::regclass AND contype = 'c'
		  AND pg_get_constraintdef(oid) LIKE '%rejected >= 0%'`); got != 1 {
		t.Errorf("the rejected >= 0 check appears %d times, want 1", got)
	}
}

// R6: migration 0005 puts evidence_run_guard() on the three evidence tables as one
// BEFORE INSERT row trigger each (tgtype 7: row, before, insert) and on no other table.
func TestMigration0005Shape(t *testing.T) {
	_, pool := openStore(t)

	if got := count(t, pool, `SELECT count(*) FROM pg_proc WHERE proname = 'evidence_run_guard'`); got != 1 {
		t.Fatalf("evidence_run_guard() functions = %d, want 1", got)
	}
	if got := count(t, pool, `
		SELECT count(*) FROM pg_trigger t
		JOIN pg_proc p ON p.oid = t.tgfoid
		WHERE NOT t.tgisinternal AND p.proname = 'evidence_run_guard'`); got != 3 {
		t.Fatalf("triggers that call evidence_run_guard() = %d, want 3", got)
	}
	for _, table := range []string{"raw_documents", "raw_search_pages", "download_attempts"} {
		if got := count(t, pool, `
			SELECT count(*) FROM pg_trigger t
			JOIN pg_proc p ON p.oid = t.tgfoid
			WHERE NOT t.tgisinternal AND p.proname = 'evidence_run_guard'
			  AND t.tgrelid = $1::regclass AND t.tgtype = 7`, table); got != 1 {
			t.Errorf("%s: %d BEFORE INSERT FOR EACH ROW trigger(s) call evidence_run_guard(), want 1", table, got)
		}
	}
}

// O1: migration 0006 adds the redaction mechanism, and this is its shape, read back
// from the catalog. Behaviour is tested in internal/db (redaction_*_db_test.go).
//   - raw_documents.redacted_at, nullable with no default;
//   - the two column checks that forbid an erased body replaced under their old names, so
//     they hold only while redacted_at is NULL, and a third that makes a redacted body empty;
//   - the tombstone table raw_redactions: one row per document, a foreign key to the
//     document, append-only by two triggers (BEFORE UPDATE OR DELETE on each row, BEFORE
//     TRUNCATE on the statement), no column that could hold a body;
//   - the trigger raw_documents_append_only kept under its name and now calling the guard
//     that admits the redaction, and a BEFORE INSERT trigger that refuses a redacted row;
//   - the function anbud_redact_raw_document: SECURITY DEFINER, a pinned search path with
//     pg_temp last, an ACL that has no PUBLIC entry;
//   - the view raw_documents_live: the same columns as the table, the rows not redacted.
func TestMigration0006Shape(t *testing.T) {
	_, pool := openStore(t)

	if got := count(t, pool, `
		SELECT count(*) FROM information_schema.columns
		WHERE table_schema = current_schema() AND table_name = 'raw_documents' AND column_name = 'redacted_at'
		  AND data_type = 'timestamp with time zone' AND is_nullable = 'YES' AND column_default IS NULL`); got != 1 {
		t.Fatalf("raw_documents.redacted_at is not a nullable TIMESTAMPTZ with no default (matches %d)", got)
	}

	for name, mustMention := range map[string]string{
		"raw_documents_body_check":        "redacted_at IS NOT NULL",
		"raw_documents_check":             "redacted_at IS NOT NULL",
		"raw_documents_redacted_is_empty": "redacted_at IS NULL",
	} {
		if got := count(t, pool, `
			SELECT count(*) FROM pg_constraint
			WHERE conrelid = 'raw_documents'::regclass AND contype = 'c' AND conname = $1
			  AND pg_get_constraintdef(oid) LIKE '%' || $2 || '%'`, name, mustMention); got != 1 {
			t.Errorf("constraint %s: %d definition(s) mention %q, want exactly 1 constraint that does", name, got, mustMention)
		}
	}
	// The format of the hash is checked for every row, redacted or not.
	if got := count(t, pool, `
		SELECT count(*) FROM pg_constraint
		WHERE conrelid = 'raw_documents'::regclass AND conname = 'raw_documents_check'
		  AND pg_get_constraintdef(oid) LIKE '%body_sha256 ~ ''^[0-9a-f]{64}$''%'`); got != 1 {
		t.Errorf("raw_documents_check lost the format test of the hash (matches %d)", got)
	}
	// Seven CHECKs: source, doffin_id, data_class, content_type, body, the hash, and the empty redacted body.
	if got := count(t, pool, `SELECT count(*) FROM pg_constraint WHERE conrelid = 'raw_documents'::regclass AND contype = 'c'`); got != 7 {
		t.Errorf("raw_documents has %d CHECK constraints, want 7", got)
	}

	if got := count(t, pool, `
		SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
		WHERE n.nspname = current_schema() AND c.relkind = 'r' AND c.relname = 'raw_redactions'`); got != 1 {
		t.Fatalf("the table raw_redactions does not exist (%d)", got)
	}
	if got := scalarText(t, pool, `
		SELECT string_agg(column_name, ',' ORDER BY ordinal_position) FROM information_schema.columns
		WHERE table_schema = current_schema() AND table_name = 'raw_redactions'`); got != "id,raw_document_id,doffin_id,original_sha256,original_size,reason,request_ref,redacted_by,redacted_at" {
		t.Errorf("raw_redactions columns = %s", got)
	}
	if got := count(t, pool, `
		SELECT count(*) FROM pg_constraint
		WHERE conrelid = 'raw_redactions'::regclass AND contype = 'u' AND conname = 'raw_redactions_one_per_document'
		  AND pg_get_constraintdef(oid) = 'UNIQUE (raw_document_id)'`); got != 1 {
		t.Errorf("raw_redactions is not unique on raw_document_id (matches %d)", got)
	}
	if got := count(t, pool, `
		SELECT count(*) FROM pg_constraint
		WHERE conrelid = 'raw_redactions'::regclass AND contype = 'f' AND confrelid = 'raw_documents'::regclass`); got != 1 {
		t.Errorf("raw_redactions has %d foreign keys to raw_documents, want 1", got)
	}

	// tgtype: 1 row, 2 before, 4 insert, 8 delete, 16 update, 32 truncate.
	for _, tr := range []struct {
		table, name, function string
		tgtype                int
	}{
		{"raw_redactions", "raw_redactions_append_only", "raw_append_only_guard", 27},
		{"raw_redactions", "raw_redactions_no_truncate", "raw_append_only_guard", 34},
		{"raw_documents", "raw_documents_append_only", "raw_documents_change_guard", 27},
		{"raw_documents", "raw_documents_no_redacted_insert", "raw_documents_insert_guard", 7},
		{"raw_documents", "catalogue_raw_changed", "catalogue_source_changed", 21},
	} {
		if got := count(t, pool, `
			SELECT count(*) FROM pg_trigger t JOIN pg_proc p ON p.oid = t.tgfoid
			WHERE NOT t.tgisinternal AND t.tgrelid = $1::regclass AND t.tgname = $2 AND p.proname = $3 AND t.tgtype = $4::smallint`,
			tr.table, tr.name, tr.function, tr.tgtype); got != 1 {
			t.Errorf("trigger %s on %s (function %s, tgtype %d): %d match(es), want 1", tr.name, tr.table, tr.function, tr.tgtype, got)
		}
	}
	// Nothing else fires on raw_documents: the UPDATE and DELETE guard under its old name and
	// the TRUNCATE guard (0002), the run guard (0005) and the INSERT guard (0006).
	if got := count(t, pool, `SELECT count(*) FROM pg_trigger WHERE NOT tgisinternal AND tgrelid = 'raw_documents'::regclass`); got != 5 {
		t.Errorf("raw_documents has %d triggers, want 5 (append_only, no_truncate, run_guard, no_redacted_insert, catalogue_raw_changed)", got)
	}

	if got := count(t, pool, `
		SELECT count(*) FROM pg_proc p
		WHERE p.oid = 'anbud_redact_raw_document(bigint, text, text)'::regprocedure
		  AND p.prosecdef AND p.prorettype = 'int8'::regtype
		  AND p.proconfig = ARRAY['search_path=pg_catalog, public, pg_temp']
		  AND p.proacl IS NOT NULL
		  AND NOT EXISTS (SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee = 0)`); got != 1 {
		t.Errorf("anbud_redact_raw_document is not SECURITY DEFINER with a pinned path and no PUBLIC grant (matches %d)", got)
	}
	for _, fn := range []string{"raw_documents_change_guard", "raw_documents_insert_guard"} {
		if got := count(t, pool, `SELECT count(*) FROM pg_proc WHERE proname = $1 AND proconfig = ARRAY['search_path=pg_catalog, public, pg_temp']`, fn); got != 1 {
			t.Errorf("%s does not pin its search path (matches %d)", fn, got)
		}
	}

	if got := count(t, pool, `
		SELECT count(*) FROM pg_views
		WHERE schemaname = current_schema() AND viewname = 'raw_documents_live' AND definition LIKE '%redacted_at IS NULL%'`); got != 1 {
		t.Fatalf("the view raw_documents_live does not select the rows that are not redacted (matches %d)", got)
	}
	columns := func(relation string) string {
		return scalarText(t, pool, `
			SELECT string_agg(column_name, ',' ORDER BY ordinal_position) FROM information_schema.columns
			WHERE table_schema = current_schema() AND table_name = $1`, relation)
	}
	if view, table := columns("raw_documents_live"), columns("raw_documents"); view != table {
		t.Errorf("raw_documents_live has the columns %s, want the table's, %s: a reader must be able to swap one for the other", view, table)
	}
}

// T64: no tenant column in the new tables (constraint 4). The scan first checks
// that it saw the columns, so it cannot pass on an empty result.
func TestNoTenantColumns(t *testing.T) {
	_, pool := openStore(t)

	tables := []string{"sync_runs", "raw_search_pages", "raw_documents", "download_attempts", "sync_days", "raw_redactions"}
	if got := count(t, pool, `
		SELECT count(*) FROM information_schema.columns
		WHERE table_schema = current_schema() AND table_name::text = ANY($1::text[])`,
		tables); got < 40 {
		t.Fatalf("saw %d columns across the raw store tables, expected the full schema", got)
	}
	if got := count(t, pool, `
		SELECT count(*) FROM information_schema.columns
		WHERE table_schema = current_schema()
		  AND table_name::text = ANY($1::text[])
		  AND column_name::text IN ('tenant_id', 'org_id', 'organization_id')`,
		tables); got != 0 {
		t.Fatalf("%d tenant column(s) in the raw store tables, want none", got)
	}
}

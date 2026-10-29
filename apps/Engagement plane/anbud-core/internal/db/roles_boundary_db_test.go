package db_test

import (
	"context"
	"fmt"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// The append-only triggers are a boundary only if the role the application runs
// as cannot take them away. These tests connect as anbud_app and try. Every
// refusal below is the server's (SQLSTATE 42501, insufficient_privilege), which
// the privileges give; the triggers' own refusals have other codes (23001 for the
// append-only guard, 55000 for the run guard), so a 42501 is never a trigger's.
// Needs a throwaway server: run with make test-db.

const (
	stateOK         = "ok"
	stateDenied     = "42501" // insufficient_privilege
	stateRestrict   = "23001" // restrict_violation: the append-only trigger
	stateNotReady   = "55000" // object_not_in_prerequisite_state: the run guard
	seedPageSQL     = `INSERT INTO raw_search_pages (source, query_key, page, content_type, body, body_sha256, fetched_at, run_id) VALUES ('doffin', 'q', 1, 'application/json', $2::bytea, encode(sha256($2::bytea), 'hex'), now(), $1)`
	seedDocumentSQL = `INSERT INTO raw_documents (source, doffin_id, content_type, body, body_sha256, fetched_at, run_id) VALUES ('doffin', '2024-000001', 'application/xml', $2::bytea, encode(sha256($2::bytea), 'hex'), now(), $1) RETURNING id`
	seedAttemptSQL  = `INSERT INTO download_attempts (run_id, source, doffin_id, attempted_at, outcome, raw_document_id) VALUES ($1, 'doffin', '2024-000001', now(), 'stored', $2)`
	seedDaySQL      = `INSERT INTO sync_days (source, day, state, last_run_id) VALUES ('doffin', '2024-01-02', 'running', $1)`
	beginRunSQL     = `INSERT INTO sync_runs (mode, range_from, range_to) VALUES ('sync', '2024-01-02', '2024-01-02') RETURNING id`
	finishRunSQL    = `UPDATE sync_runs SET state = 'done', finished_at = now() WHERE id = $1`
	finishDaySQL    = `UPDATE sync_days SET state = 'searched', num_total = 1, num_accessible = 1, updated_at = now() WHERE source = 'doffin' AND day = '2024-01-02'`
	seedPageBody    = `{"hits":[]}`
)

// seedAsApp writes one of each kind of row as the application does, under a run
// that is running, and returns the run's id.
func seedAsApp(ctx context.Context, t *testing.T, app *pgxpool.Pool) int64 {
	t.Helper()
	var runID, docID int64
	if err := app.QueryRow(ctx, beginRunSQL).Scan(&runID); err != nil {
		t.Fatalf("begin a run as the app role: %s", dbtest.Describe(err))
	}
	if _, err := app.Exec(ctx, seedPageSQL, runID, []byte(seedPageBody)); err != nil {
		t.Fatalf("insert a search page as the app role: %s", dbtest.Describe(err))
	}
	if err := app.QueryRow(ctx, seedDocumentSQL, runID, []byte("<notice/>")).Scan(&docID); err != nil {
		t.Fatalf("insert a notice as the app role: %s", dbtest.Describe(err))
	}
	if _, err := app.Exec(ctx, seedAttemptSQL, runID, docID); err != nil {
		t.Fatalf("insert an attempt as the app role: %s", dbtest.Describe(err))
	}
	if _, err := app.Exec(ctx, seedDaySQL, runID); err != nil {
		t.Fatalf("insert a day as the app role: %s", dbtest.Describe(err))
	}
	return runID
}

// The application's whole write path works with these privileges and no others:
// the run ledger is inserted and updated, evidence is inserted and read back, and
// the identity columns need no sequence privilege.
func TestAppRoleWritesEvidenceAndLedger(t *testing.T) {
	rs, _ := migratedRoles(t)
	ctx := roleCtx(t)
	app := dbtest.Pool(t, rs.AppDSN)

	runID := seedAsApp(ctx, t, app)

	if got := outcome(ctx, app, finishDaySQL); got != stateOK {
		t.Errorf("update of a sync day as the app role = %s, want ok", got)
	}
	if got := outcome(ctx, app, `INSERT INTO sync_days (source, day, state, last_run_id) VALUES ('doffin', '2024-01-03', 'running', $1) ON CONFLICT (source, day) DO UPDATE SET state = EXCLUDED.state`, runID); got != stateOK {
		t.Errorf("upsert of a sync day as the app role = %s, want ok", got)
	}
	if got := outcome(ctx, app, `INSERT INTO raw_documents (source, doffin_id, content_type, body, body_sha256, fetched_at, run_id) VALUES ('doffin', '2024-000001', 'application/xml', $2::bytea, encode(sha256($2::bytea), 'hex'), now(), $1) ON CONFLICT (source, doffin_id, body_sha256) DO NOTHING`, runID, []byte("<notice/>")); got != stateOK {
		t.Errorf("INSERT ... ON CONFLICT DO NOTHING as the app role = %s, want ok", got)
	}
	for _, table := range allTables[:5] {
		var n int
		if err := app.QueryRow(ctx, `SELECT count(*) FROM `+table).Scan(&n); err != nil || n == 0 {
			t.Errorf("the app role reads %d rows of %s (error %s), want some", n, table, dbtest.Describe(err))
		}
	}
	if got := outcome(ctx, app, finishRunSQL, runID); got != stateOK {
		t.Errorf("finishing the run as the app role = %s, want ok", got)
	}

	// The budget lock the sync takes is a session advisory lock.
	var held bool
	if err := app.QueryRow(ctx, `SELECT pg_try_advisory_lock(42)`).Scan(&held); err != nil || !held {
		t.Errorf("the app role cannot take an advisory lock (error %s)", dbtest.Describe(err))
	}
}

// The run guard (migration 0005) locks the run's row with FOR SHARE, and a row lock
// needs UPDATE on that table, which the ledger grant gives. With the grant the guard
// lets a running run's evidence in and refuses a finished run's with its own code,
// 55000. Without it every evidence insert would fail on the server's 42501.
func TestRunGuardWorksForTheAppRoleBecauseTheLedgerGrantsUpdate(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	app := dbtest.Pool(t, rs.AppDSN)

	runID := seedAsApp(ctx, t, app)
	if got := outcome(ctx, app, seedPageSQL, runID, []byte(`{"hits":[1]}`)); got != stateOK {
		t.Fatalf("an evidence insert under a running run = %s, want ok: FOR SHARE must work for the app role", got)
	}

	// Control: take the ledger's UPDATE away, and the same insert is refused by the
	// server, because the guard cannot lock the run.
	if _, err := admin.Exec(ctx, `REVOKE UPDATE ON sync_runs FROM anbud_app`); err != nil {
		t.Fatalf("revoke: %s", dbtest.Describe(err))
	}
	if got := outcome(ctx, app, seedPageSQL, runID, []byte(`{"hits":[2]}`)); got != stateDenied {
		t.Errorf("an evidence insert without UPDATE on sync_runs = %s, want %s", got, stateDenied)
	}
	if _, err := admin.Exec(ctx, `GRANT UPDATE ON sync_runs TO anbud_app`); err != nil {
		t.Fatalf("grant: %s", dbtest.Describe(err))
	}

	// A finished run takes no more evidence, and says so in its own words.
	if got := outcome(ctx, app, finishRunSQL, runID); got != stateOK {
		t.Fatalf("finish = %s, want ok", got)
	}
	if got := outcome(ctx, app, seedPageSQL, runID, []byte(`{"hits":[3]}`)); got != stateNotReady {
		t.Errorf("an evidence insert under a finished run = %s, want %s from the run guard", got, stateNotReady)
	}
}

// UPDATE, DELETE and TRUNCATE on the evidence tables are refused by the server, with
// rows present, so the refusal cannot be a trigger finding nothing to do.
func TestAppRoleCannotChangeEvidence(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	app := dbtest.Pool(t, rs.AppDSN)
	runID := seedAsApp(ctx, t, app)

	update := map[string]string{
		"raw_search_pages":  `UPDATE raw_search_pages SET content_type = 'x'`,
		"raw_documents":     `UPDATE raw_documents SET content_type = 'x'`,
		"download_attempts": `UPDATE download_attempts SET message = 'x'`,
	}
	for _, table := range evidenceTables {
		var n int
		if err := admin.QueryRow(ctx, `SELECT count(*) FROM `+table).Scan(&n); err != nil || n == 0 {
			t.Fatalf("%s has %d rows to protect (error %s), want some", table, n, dbtest.Describe(err))
		}
		for name, sql := range map[string]string{
			"UPDATE":   update[table],
			"DELETE":   `DELETE FROM ` + table,
			"TRUNCATE": `TRUNCATE ` + table,
		} {
			if got := outcome(ctx, app, sql); got != stateDenied {
				t.Errorf("%s on %s as the app role = %s, want %s", name, table, got, stateDenied)
			}
		}
		if err := admin.QueryRow(ctx, `SELECT count(*) FROM `+table).Scan(&n); err != nil || n == 0 {
			t.Errorf("%s has %d rows after the attempts, want them all", table, n)
		}
	}

	// Statements that change or lock evidence without being a plain UPDATE, DELETE or TRUNCATE:
	// an upsert that updates, a MERGE that updates or deletes, a row lock (every kind of it needs
	// UPDATE) and a table lock that excludes the writers. Each is refused by the server.
	evidenceStatements := evidenceBypasses(runID)
	for _, table := range evidenceTables {
		for name, sql := range evidenceStatements(table) {
			if got := outcome(ctx, app, sql); got != stateDenied {
				t.Errorf("%s on %s as the app role = %s, want %s", name, table, got, stateDenied)
			}
		}
		for name, mode := range map[string]string{"LOCK TABLE ... IN ACCESS EXCLUSIVE MODE": "ACCESS EXCLUSIVE", "LOCK TABLE ... IN EXCLUSIVE MODE": "EXCLUSIVE"} {
			if got := outcomeInTx(ctx, app, `LOCK TABLE `+table+` IN `+mode+` MODE`); got != stateDenied {
				t.Errorf("%s on %s as the app role = %s, want %s", name, table, got, stateDenied)
			}
		}
		// The same statements are not refused by the privilege layer for a superuser: the refusals
		// above come from the role, and none of them is a statement that cannot be run.
		for name, sql := range evidenceStatements(table) {
			if got := outcomeInTx(ctx, admin, sql); got == stateDenied {
				t.Errorf("%s on %s as a superuser = %s: the control is void", name, table, got)
			}
		}
		if got := outcomeInTx(ctx, admin, `LOCK TABLE `+table+` IN ACCESS EXCLUSIVE MODE`); got != stateOK {
			t.Errorf("LOCK TABLE on %s as a superuser = %s, want ok: the control is void", table, got)
		}
	}

	// The ledger may be updated, never emptied.
	for _, table := range ledgerTables {
		for name, sql := range map[string]string{"DELETE": `DELETE FROM ` + table, "TRUNCATE": `TRUNCATE ` + table} {
			if got := outcome(ctx, app, sql); got != stateDenied {
				t.Errorf("%s on %s as the app role = %s, want %s", name, table, got, stateDenied)
			}
		}
	}
	if got := outcome(ctx, app, `UPDATE sync_runs SET mode = 'census'`); got != stateRestrict {
		// UPDATE is granted on the ledger, and the provenance trigger still refuses a changed mode.
		t.Errorf("a change to a run's provenance as the app role = %s, want the trigger's %s", got, stateRestrict)
	}
	if got := outcome(ctx, app, `UPDATE schema_migrations SET name = name`); got != stateDenied {
		t.Errorf("UPDATE of schema_migrations as the app role = %s, want %s", got, stateDenied)
	}
	if got := outcome(ctx, app, `INSERT INTO schema_migrations (version, name) VALUES (99, 'forged')`); got != stateDenied {
		t.Errorf("INSERT into schema_migrations as the app role = %s, want %s", got, stateDenied)
	}
	if got := outcome(ctx, app, `SELECT version FROM schema_migrations`); got != stateDenied {
		t.Errorf("SELECT of schema_migrations as the app role = %s, want %s", got, stateDenied)
	}
}

// Everything that would take the triggers away, or the tables, or the role's own
// limits, is refused: the application is not the owner and holds no CREATE.
func TestAppRoleCannotChangeTheSchemaOrItself(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	app := dbtest.Pool(t, rs.AppDSN)
	seedAsApp(ctx, t, app)

	const guard = `CREATE OR REPLACE FUNCTION raw_append_only_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END; $$`
	statements := []struct{ name, sql string }{
		{"CREATE TABLE", `CREATE TABLE anbud_probe (id int)`},
		{"CREATE SCHEMA", `CREATE SCHEMA anbud_probe`},
		{"CREATE VIEW", `CREATE VIEW anbud_probe AS SELECT 1`},
		{"DROP TRIGGER", `DROP TRIGGER raw_documents_append_only ON raw_documents`},
		{"DROP the run guard trigger", `DROP TRIGGER raw_documents_run_guard ON raw_documents`},
		{"DISABLE TRIGGER", `ALTER TABLE raw_documents DISABLE TRIGGER raw_documents_append_only`},
		{"DISABLE TRIGGER ALL", `ALTER TABLE raw_documents DISABLE TRIGGER ALL`},
		{"DISABLE TRIGGER USER", `ALTER TABLE raw_documents DISABLE TRIGGER USER`},
		{"ENABLE ALWAYS TRIGGER", `ALTER TABLE raw_documents ENABLE ALWAYS TRIGGER raw_documents_append_only`},
		{"CREATE TRIGGER", `CREATE TRIGGER anbud_probe BEFORE INSERT ON raw_documents FOR EACH ROW EXECUTE FUNCTION raw_append_only_guard()`},
		{"DROP TABLE", `DROP TABLE raw_documents CASCADE`},
		{"ALTER TABLE", `ALTER TABLE raw_documents ADD COLUMN anbud_probe int`},
		{"ALTER TABLE OWNER", `ALTER TABLE raw_documents OWNER TO anbud_app`},
		{"RENAME", `ALTER TABLE raw_documents RENAME TO anbud_probe`},
		{"REPLACE the append-only guard", guard},
		{"DROP FUNCTION", `DROP FUNCTION raw_append_only_guard() CASCADE`},
		{"COMMENT", `COMMENT ON TABLE raw_documents IS 'x'`},
		{"session_replication_role", `SET session_replication_role = replica`},
		{"ALTER ROLE SUPERUSER", `ALTER ROLE anbud_app SUPERUSER`},
		{"ALTER ROLE CREATEROLE", `ALTER ROLE anbud_app CREATEROLE`},
		{"CREATE ROLE", `CREATE ROLE anbud_probe`},
		{"SET ROLE the owner", `SET ROLE anbud_migrator`},
		{"SET SESSION AUTHORIZATION", `SET SESSION AUTHORIZATION anbud_migrator`},
		{"COPY TO PROGRAM", `COPY (SELECT 1) TO PROGRAM 'true'`},
		{"ALTER DATABASE", `ALTER DATABASE ` + `"` + rs.Database + `" SET search_path = pg_temp`},
		{"ALTER DEFAULT PRIVILEGES FOR the owner", `ALTER DEFAULT PRIVILEGES FOR ROLE anbud_migrator GRANT ALL ON TABLES TO anbud_app`},

		// Migration 0006: the redaction mechanism is the owner's to change, and so is
		// everything its guards stand on.
		{"CREATE OR REPLACE the redaction function", `CREATE OR REPLACE FUNCTION anbud_redact_raw_document(p_id bigint, p_reason text, p_request text) RETURNS bigint LANGUAGE sql AS 'SELECT 1::bigint'`},
		{"DROP the redaction function", `DROP FUNCTION anbud_redact_raw_document(bigint, text, text)`},
		{"ALTER the redaction function to SECURITY INVOKER", `ALTER FUNCTION anbud_redact_raw_document(bigint, text, text) SECURITY INVOKER`},
		{"ALTER the redaction function's search path", `ALTER FUNCTION anbud_redact_raw_document(bigint, text, text) SET search_path = pg_temp`},
		{"ALTER the redaction function's owner", `ALTER FUNCTION anbud_redact_raw_document(bigint, text, text) OWNER TO anbud_app`},
		{"REPLACE the guard on raw_documents", `CREATE OR REPLACE FUNCTION raw_documents_change_guard() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END; $$`},
		{"DROP the INSERT guard trigger", `DROP TRIGGER raw_documents_no_redacted_insert ON raw_documents`},
		{"DISABLE the tombstones' trigger", `ALTER TABLE raw_redactions DISABLE TRIGGER raw_redactions_append_only`},
		{"DISABLE the tombstones' TRUNCATE trigger", `ALTER TABLE raw_redactions DISABLE TRIGGER raw_redactions_no_truncate`},
		{"DROP a relaxed check", `ALTER TABLE raw_documents DROP CONSTRAINT raw_documents_redacted_is_empty`},
		{"DROP the tombstones", `DROP TABLE raw_redactions`},
		{"DROP the view", `DROP VIEW raw_documents_live`},
		{"REPLACE the view", `CREATE OR REPLACE VIEW raw_documents_live AS SELECT * FROM raw_documents`},
	}
	for _, s := range statements {
		if got := outcome(ctx, app, s.sql); got != stateDenied {
			t.Errorf("%s as the app role = %s, want %s", s.name, got, stateDenied)
		}
	}

	// The role cannot give itself what it lacks. A GRANT by a role without the grant
	// option is a warning, not an error, so the check is that nothing changed.
	for _, sql := range []string{
		`GRANT UPDATE, DELETE, TRUNCATE ON raw_documents TO anbud_app`,
		`GRANT ALL ON ALL TABLES IN SCHEMA public TO anbud_app`,
		`GRANT CREATE ON SCHEMA public TO anbud_app`,
		`GRANT EXECUTE ON FUNCTION anbud_redact_raw_document(bigint, text, text) TO anbud_app`,
		`GRANT INSERT ON raw_redactions TO anbud_app`,
	} {
		_, _ = app.Exec(ctx, sql)
	}
	for _, sql := range []string{
		`UPDATE raw_documents SET content_type = 'x'`, `DELETE FROM raw_documents`, `TRUNCATE raw_documents`, `CREATE TABLE anbud_probe (id int)`,
		`SELECT anbud_redact_raw_document(1, 'erasure_request', 'T-1')`,
		`INSERT INTO raw_redactions (raw_document_id, doffin_id, original_sha256, original_size, reason, request_ref, redacted_by) VALUES (1, '2024-000001', repeat('a', 64), 9, 'objection', 'T-1', 'x')`,
	} {
		if got := outcome(ctx, app, sql); got != stateDenied {
			t.Errorf("%q after the app role tried to grant itself the privilege = %s, want %s", sql, got, stateDenied)
		}
	}
	if got := grantedMatrix(ctx, t, admin); !equalMatrix(got, expectedMatrix()) {
		t.Errorf("the privilege matrix changed after the app role's attempts:\n%s", formatMatrix(got))
	}

	// The same statements are allowed to the owner and to a superuser: the refusals
	// above come from the role, not from a statement that cannot be run at all.
	tx, err := admin.Begin(ctx)
	if err != nil {
		t.Fatalf("begin: %s", dbtest.Describe(err))
	}
	defer func() { _ = tx.Rollback(ctx) }()
	for _, sql := range []string{
		`ALTER TABLE raw_documents DISABLE TRIGGER raw_documents_append_only`,
		`SET LOCAL session_replication_role = replica`,
		guard,
	} {
		if _, err := tx.Exec(ctx, sql); err != nil {
			t.Errorf("a superuser cannot run %q (%s): the control is void", sql, dbtest.Describe(err))
		}
	}
}

// outcomeInTx runs one statement inside a transaction that is rolled back, and says how it ended.
// LOCK TABLE needs a transaction block, and asks for the privilege only after finding one.
func outcomeInTx(ctx context.Context, pool *pgxpool.Pool, sql string) string {
	tx, err := pool.Begin(ctx)
	if err != nil {
		return dbtest.Outcome(err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	_, err = tx.Exec(ctx, sql)
	return dbtest.Outcome(err)
}

// evidenceBypasses are statements that would change or lock evidence by a road other than a plain
// UPDATE, DELETE or TRUNCATE, for the given table. Each names only what the application's own
// privileges (SELECT and INSERT) decide: the server refuses it before any row is looked at.
func evidenceBypasses(runID int64) func(table string) map[string]string {
	column := map[string]string{"raw_search_pages": "content_type", "raw_documents": "content_type", "download_attempts": "message"}
	insert := map[string]string{
		"raw_search_pages":  fmt.Sprintf(`INSERT INTO raw_search_pages (source, query_key, page, content_type, body, body_sha256, fetched_at, run_id) VALUES ('doffin', 'q', 99, 'application/json', 'x'::bytea, encode(sha256('x'::bytea), 'hex'), now(), %d)`, runID),
		"raw_documents":     fmt.Sprintf(`INSERT INTO raw_documents (source, doffin_id, content_type, body, body_sha256, fetched_at, run_id) VALUES ('doffin', '2024-000009', 'application/xml', 'x'::bytea, encode(sha256('x'::bytea), 'hex'), now(), %d)`, runID),
		"download_attempts": fmt.Sprintf(`INSERT INTO download_attempts (run_id, source, doffin_id, attempted_at, outcome, raw_document_id) VALUES (%d, 'doffin', '2024-000009', now(), 'stored', 1)`, runID),
	}
	return func(table string) map[string]string {
		col := column[table]
		merge := `MERGE INTO ` + table + ` t USING (SELECT 1 AS k) s ON t.id = -1 WHEN MATCHED THEN `
		return map[string]string{
			"INSERT ... ON CONFLICT DO UPDATE":   insert[table] + ` ON CONFLICT (id) DO UPDATE SET ` + col + ` = 'x'`,
			"MERGE ... WHEN MATCHED THEN UPDATE": merge + `UPDATE SET ` + col + ` = 'x'`,
			"MERGE ... WHEN MATCHED THEN DELETE": merge + `DELETE`,
			"SELECT ... FOR UPDATE":              `SELECT id FROM ` + table + ` FOR UPDATE`,
			"SELECT ... FOR NO KEY UPDATE":       `SELECT id FROM ` + table + ` FOR NO KEY UPDATE`,
			"SELECT ... FOR SHARE":               `SELECT id FROM ` + table + ` FOR SHARE`,
			"SELECT ... FOR KEY SHARE":           `SELECT id FROM ` + table + ` FOR KEY SHARE`,
		}
	}
}

func equalMatrix(a, b map[string]map[string][]string) bool {
	return formatMatrix(a) == formatMatrix(b)
}

// The migrations cannot be run as the application: the runner creates its own
// bookkeeping table, which needs CREATE on the schema. With one DSN and the app
// role the service stops at start up, which is why ANBUD_MIGRATE_DSN exists.
func TestAppRoleCannotRunMigrations(t *testing.T) {
	rs, _ := migratedRoles(t)
	ctx := roleCtx(t)
	app := dbtest.Pool(t, rs.AppDSN)

	err := db.ApplyMigrations(ctx, app)
	if err == nil {
		t.Fatal("ApplyMigrations succeeded as the app role")
	}
	if got := dbtest.Outcome(err); got != stateDenied {
		t.Errorf("ApplyMigrations as the app role: %s, want %s", got, stateDenied)
	}
}

// A login role in anbud_dba reads every table and writes nothing: the group role
// is what the privileges are granted to, and the human's own role inherits it.
func TestDBARoleReadsEverythingAndWritesNothing(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	app := dbtest.Pool(t, rs.AppDSN)
	seedAsApp(ctx, t, app)

	if scanBool(ctx, t, admin, `SELECT rolcanlogin FROM pg_roles WHERE rolname = 'anbud_dba'`) {
		t.Error("anbud_dba can log in; it is a group role")
	}
	dba := dbtest.Pool(t, dbtest.CreateLoginRole(t, rs, "anbud_test_dba_user", "", dbtest.RoleDBA))

	for _, table := range allTables {
		if got := outcome(ctx, dba, `SELECT count(*) FROM `+table); got != stateOK {
			t.Errorf("SELECT on %s as a member of anbud_dba = %s, want ok", table, got)
		}
	}
	for _, table := range evidenceTables {
		for name, sql := range map[string]string{"DELETE": `DELETE FROM ` + table, "TRUNCATE": `TRUNCATE ` + table} {
			if got := outcome(ctx, dba, sql); got != stateDenied {
				t.Errorf("%s on %s as a member of anbud_dba = %s, want %s", name, table, got, stateDenied)
			}
		}
	}
	for _, sql := range []string{
		`INSERT INTO sync_runs (mode, range_from, range_to) VALUES ('sync', '2024-01-02', '2024-01-02')`,
		`UPDATE sync_runs SET last_error = 'x'`,
		`UPDATE raw_documents SET content_type = 'x'`,
		`CREATE TABLE anbud_probe (id int)`,
		`ALTER TABLE raw_documents DISABLE TRIGGER raw_documents_append_only`,

		// Migration 0006: the group may run the redaction function, and may do nothing
		// else to the mechanism or to what it writes.
		`INSERT INTO raw_redactions (raw_document_id, doffin_id, original_sha256, original_size, reason, request_ref, redacted_by) VALUES (1, '2024-000001', repeat('a', 64), 9, 'objection', 'T-1', 'x')`,
		`UPDATE raw_redactions SET reason = 'objection'`,
		`DELETE FROM raw_redactions`,
		`TRUNCATE raw_redactions`,
		`ALTER TABLE raw_redactions DISABLE TRIGGER raw_redactions_append_only`,
		`ALTER FUNCTION anbud_redact_raw_document(bigint, text, text) SET search_path = pg_temp`,
		`ALTER FUNCTION anbud_redact_raw_document(bigint, text, text) SECURITY INVOKER`,
		`DROP FUNCTION anbud_redact_raw_document(bigint, text, text)`,
		`CREATE OR REPLACE FUNCTION anbud_redact_raw_document(p_id bigint, p_reason text, p_request text) RETURNS bigint LANGUAGE sql AS 'SELECT 1::bigint'`,
		`UPDATE raw_documents_live SET content_type = 'x'`,
	} {
		if got := outcome(ctx, dba, sql); got != stateDenied {
			t.Errorf("%q as a member of anbud_dba = %s, want %s", sql, got, stateDenied)
		}
	}
}

// session_replication_role = replica is how a superuser stops every trigger of a session, the
// append-only ones included. It is a setting only a superuser may change, and a member of the DBA
// group, which holds the redaction function and nothing that changes evidence, is refused with the
// server's own code however the setting is reached.
func TestDBAGroupCannotSwitchOffTheTriggersWithSessionReplicationRole(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	conn, err := env.dba.Acquire(ctx)
	if err != nil {
		t.Fatalf("acquire as a member of anbud_dba: %s", dbtest.Describe(err))
	}
	defer func() {
		_, _ = conn.Exec(context.Background(), "DISCARD ALL")
		conn.Release()
	}()

	for name, sql := range map[string]string{
		"SET session_replication_role":         `SET session_replication_role = replica`,
		"SET SESSION session_replication_role": `SET SESSION session_replication_role = replica`,
		"set_config":                           `SELECT set_config('session_replication_role', 'replica', false)`,
		"ALTER ROLE ... SET":                   `ALTER ROLE ` + ident(dbaLogin) + ` SET session_replication_role = replica`,
	} {
		_, err := conn.Exec(ctx, sql)
		if got := dbtest.Outcome(err); got != stateDenied {
			t.Errorf("%s as a member of anbud_dba = %s, want %s", name, got, stateDenied)
		}
	}
	var mode string
	if err := conn.QueryRow(ctx, `SHOW session_replication_role`).Scan(&mode); err != nil || mode != "origin" {
		t.Errorf("session_replication_role is %q after the attempts (error %s), want origin", mode, dbtest.Describe(err))
	}

	// The control: a superuser may, so the refusals come from the role.
	if got := outcomeInTx(ctx, env.admin, `SET LOCAL session_replication_role = replica`); got != stateOK {
		t.Errorf("SET LOCAL session_replication_role as a superuser = %s, want ok: the control is void", got)
	}
}

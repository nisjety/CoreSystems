package docstore

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// These tests run the role model (docs/ROLES.md) on a throwaway database: the migrations as the owner,
// the store as the application's role. Row level security binds a role that owns nothing, and only that
// role shows whether it holds.

type roleEnv struct {
	rs    dbtest.RoleSet
	admin *pgxpool.Pool // the server's own role: for arranging and looking
	app   *pgxpool.Pool // anbud_app: what the service runs as
	store *Store        // over app
}

func newRoleEnv(t *testing.T) roleEnv {
	t.Helper()
	rs := dbtest.RoleDatabase(t)
	migrator := dbtest.Pool(t, rs.MigratorDSN)
	if err := db.ApplyMigrations(testCtx(t), migrator); err != nil {
		t.Fatalf("ApplyMigrations as the migrator: %s", dbtest.Describe(err))
	}
	app := dbtest.Pool(t, rs.AppDSN)
	return roleEnv{rs: rs, admin: dbtest.Pool(t, rs.AdminDSN), app: app, store: New(app)}
}

func outcome(ctx context.Context, pool *pgxpool.Pool, sql string, args ...any) string {
	_, err := pool.Exec(ctx, sql, args...)
	return dbtest.Outcome(err)
}

func TestTheAppRoleCanUseTheStore(t *testing.T) {
	env := newRoleEnv(t)
	up, created, err := env.store.Create(testCtx(t), sample("org-a", "2026-100001", "a"))
	if err != nil || !created {
		t.Fatalf("Create as the app role: %s", dbtest.Describe(err))
	}
	if got, err := env.store.List(testCtx(t), "org-a", "2026-100001"); err != nil || len(got) != 1 {
		t.Errorf("List as the app role: %d, %v", len(got), err)
	}
	if ev, err := env.store.EvidenceFor(testCtx(t), "org-a", []string{"2026-100001"}); err != nil || ev["2026-100001"].Documents != 1 {
		t.Errorf("EvidenceFor as the app role: %+v, %v", ev, err)
	}
	if _, _, err := env.store.Counts(testCtx(t), "org-a", "2026-100001"); err != nil {
		t.Errorf("Counts as the app role: %v", err)
	}
	if ok, err := env.store.Delete(testCtx(t), "org-a", "2026-100001", up.ID); err != nil || !ok {
		t.Errorf("Delete as the app role: %v, %v", ok, err)
	}
	if _, err := env.store.PurgeExpired(testCtx(t)); err != nil {
		t.Errorf("PurgeExpired as the app role: %s", dbtest.Describe(err))
	}
}

// With row level security the database itself keeps organisations apart. A query that forgets to
// name the organisation sees nothing of the others', and a row for another organisation is refused.
func TestRowLevelSecurityKeepsOrganisationsApartWhateverTheQueryForgets(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	for _, u := range []NewUpload{sample("org-a", "2026-100001", "a"), sample("org-b", "2026-100001", "b")} {
		if _, _, err := env.store.Create(ctx, u); err != nil {
			t.Fatalf("Create: %s", dbtest.Describe(err))
		}
	}

	var withNoOrganisation int
	if err := env.app.QueryRow(ctx, `SELECT count(*) FROM tenant.tender_uploads`).Scan(&withNoOrganisation); err != nil || withNoOrganisation != 0 {
		t.Errorf("a session with no organisation sees %d rows, %v; want none", withNoOrganisation, err)
	}

	tx, err := env.app.Begin(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `SELECT set_config('anbud.org_id', 'org-a', true)`); err != nil {
		t.Fatal(err)
	}
	var seen []string
	rows, err := tx.Query(ctx, `SELECT org_id FROM tenant.tender_uploads`) // no WHERE: the policy is all there is
	if err != nil {
		t.Fatalf("query: %s", dbtest.Describe(err))
	}
	for rows.Next() {
		var org string
		_ = rows.Scan(&org)
		seen = append(seen, org)
	}
	rows.Close()
	if len(seen) != 1 || seen[0] != "org-a" {
		t.Errorf("a session for org-a sees %v, want only org-a's row", seen)
	}

	// A row for another organisation is refused: SQLSTATE 42501, the policy's check.
	_, err = tx.Exec(ctx, `
INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, expires_at)
VALUES ('org-b', '2026-100009', repeat('c', 64), 1, '[]', '[]', 0, 0, now() + interval '1 day')`)
	if got := dbtest.Outcome(err); got != "42501" {
		t.Errorf("org-a's session inserting a row of org-b: %s, want 42501", got)
	}
}

func TestADeleteWithNoConditionTouchesOnlyTheSessionsOrganisation(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	for _, u := range []NewUpload{sample("org-a", "2026-100001", "a"), sample("org-b", "2026-100001", "b")} {
		if _, _, err := env.store.Create(ctx, u); err != nil {
			t.Fatal(err)
		}
	}
	tx, err := env.app.Begin(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := tx.Exec(ctx, `SELECT set_config('anbud.org_id', 'org-a', true)`); err != nil {
		t.Fatal(err)
	}
	tag, err := tx.Exec(ctx, `DELETE FROM tenant.tender_uploads`)
	if err != nil {
		t.Fatalf("delete: %s", dbtest.Describe(err))
	}
	if err := tx.Commit(ctx); err != nil {
		t.Fatal(err)
	}
	if tag.RowsAffected() != 1 {
		t.Errorf("deleted %d rows, want org-a's one", tag.RowsAffected())
	}
	if n := count(t, env.admin, `SELECT count(*) FROM tenant.tender_uploads WHERE org_id = 'org-b'`); n != 1 {
		t.Errorf("org-b has %d rows left, want 1", n)
	}
}

func TestTheAppRoleCannotUpdateTruncateOrChangeTheTable(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	if _, _, err := env.store.Create(ctx, sample("org-a", "2026-100001", "a")); err != nil {
		t.Fatal(err)
	}
	for name, sql := range map[string]string{
		"UPDATE":           `UPDATE tenant.tender_uploads SET expires_at = expires_at + interval '1 year'`,
		"TRUNCATE":         `TRUNCATE tenant.tender_uploads`,
		"ALTER":            `ALTER TABLE tenant.tender_uploads DISABLE ROW LEVEL SECURITY`,
		"DROP POLICY":      `DROP POLICY tender_uploads_own_organisation ON tenant.tender_uploads`,
		"CREATE in tenant": `CREATE TABLE tenant.mine (id int)`,
		"a row lock":       `SELECT * FROM tenant.tender_uploads FOR UPDATE`,
	} {
		if got := outcome(ctx, env.app, sql); got != "42501" {
			t.Errorf("%s as the app role: %s, want 42501 (insufficient privilege)", name, got)
		}
	}

	// A role cannot give itself what it was not given: without the grant option, GRANT is a warning
	// and grants nothing.
	_ = outcome(ctx, env.app, `GRANT UPDATE, TRUNCATE ON tenant.tender_uploads TO anbud_app`)
	for _, priv := range []string{"UPDATE", "TRUNCATE"} {
		var has bool
		if err := env.admin.QueryRow(ctx, `SELECT has_table_privilege('anbud_app', 'tenant.tender_uploads', $1)`, priv).Scan(&has); err != nil || has {
			t.Errorf("the app role gave itself %s: %v, %v", priv, has, err)
		}
	}
}

// A session may switch row security off. The server does not then skip the policy for a role that is
// not the table's owner: it refuses the query.
func TestSwitchingRowSecurityOffMakesTheServerRefuseNotSkip(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	if _, _, err := env.store.Create(ctx, sample("org-a", "2026-100001", "a")); err != nil {
		t.Fatal(err)
	}
	conn, err := env.app.Acquire(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Release()
	if _, err := conn.Exec(ctx, `SET row_security = off`); err != nil {
		t.Fatalf("SET row_security: %s", dbtest.Describe(err))
	}
	var n int
	err = conn.QueryRow(ctx, `SELECT count(*) FROM tenant.tender_uploads`).Scan(&n)
	if got := dbtest.Outcome(err); got != "42501" {
		t.Errorf("a query with row security off: %s (%d rows), want 42501: the rows must not be shown", got, n)
	}
	// The connection goes back to the pool: leave it as it was found.
	if _, err := conn.Exec(ctx, `RESET row_security`); err != nil {
		t.Fatalf("RESET row_security: %s", dbtest.Describe(err))
	}
}

func TestOnlyTheOwnerBypassesRowLevelSecurity(t *testing.T) {
	env := newRoleEnv(t)
	for _, role := range []string{dbtest.RoleApp, dbtest.RoleDBA} {
		var bypass bool
		if err := env.admin.QueryRow(testCtx(t), `SELECT rolbypassrls OR rolsuper FROM pg_roles WHERE rolname = $1`, role).Scan(&bypass); err != nil || bypass {
			t.Errorf("%s bypasses row level security: %v, %v", role, bypass, err)
		}
	}
	var enabled, forced bool
	if err := env.admin.QueryRow(testCtx(t), `SELECT relrowsecurity, relforcerowsecurity FROM pg_class WHERE oid = 'tenant.tender_uploads'::regclass`).Scan(&enabled, &forced); err != nil || !enabled {
		t.Errorf("row level security is not enabled on the table: %v, %v", enabled, err)
	}
}

func TestTheDBAGroupAndStrangersHaveNothingOnTheTenantSchema(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	if _, _, err := env.store.Create(ctx, sample("org-a", "2026-100001", "a")); err != nil {
		t.Fatal(err)
	}
	dba := dbtest.Pool(t, dbtest.CreateLoginRole(t, env.rs, "anbud_test_dba_member", "", dbtest.RoleDBA))
	nobody := dbtest.Pool(t, dbtest.CreateLoginRole(t, env.rs, "anbud_test_nobody", ""))

	for who, pool := range map[string]*pgxpool.Pool{"a member of the DBA group": dba, "a role the script never heard of": nobody} {
		for what, sql := range map[string]string{
			"select": `SELECT count(*) FROM tenant.tender_uploads`,
			"insert": `INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, expires_at) VALUES ('o', '2026-100001', repeat('a', 64), 1, '[]', '[]', 0, 0, now() + interval '1 day')`,
			"delete": `DELETE FROM tenant.tender_uploads`,
			"purge":  `SELECT tenant.purge_expired_uploads()`,
		} {
			if got := outcome(ctx, pool, sql); got != "42501" {
				t.Errorf("%s: %s: %s, want 42501", who, what, got)
			}
		}
	}
}

// The purge is the one thing the application does across organisations, and all it does is delete
// what has expired.
func TestThePurgeRemovesExpiredRowsForEveryOrganisationAndNothingElse(t *testing.T) {
	env := newRoleEnv(t)
	insertExpired(t, env.admin, "org-a", "2026-100001", "old-a")
	insertExpired(t, env.admin, "org-b", "2026-100002", "old-b")
	if _, _, err := env.store.Create(testCtx(t), sample("org-a", "2026-100001", "new")); err != nil {
		t.Fatal(err)
	}
	removed, err := env.store.PurgeExpired(testCtx(t))
	if err != nil || removed != 2 {
		t.Errorf("PurgeExpired as the app role = %d, %v; want 2", removed, dbtest.Describe(err))
	}
	if n := count(t, env.admin, `SELECT count(*) FROM tenant.tender_uploads`); n != 1 {
		t.Errorf("rows = %d, want the one that has not expired", n)
	}
}

// An upload that has not expired is not purged, however near its expiry: the purge goes by the clock and
// not by a margin.
func TestThePurgeKeepsAnUploadThatWillExpireSoon(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	for _, in := range []string{"1 minute", "1 hour", "23 hours"} {
		_, err := env.admin.Exec(ctx, `
INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, created_at, expires_at)
VALUES ('org-a', '2026-100001', $1, 1, '[]', '[]', 0, 0, now() - interval '3 days', now() + $2::interval)`, sha(in), in)
		if err != nil {
			t.Fatalf("insert an upload that expires in %s: %s", in, dbtest.Describe(err))
		}
	}
	insertExpired(t, env.admin, "org-a", "2026-100001", "old")
	removed, err := env.store.PurgeExpired(ctx)
	if err != nil || removed != 1 {
		t.Errorf("PurgeExpired = %d, %v; want only the one that has expired", removed, dbtest.Describe(err))
	}
	if n := count(t, env.admin, `SELECT count(*) FROM tenant.tender_uploads`); n != 3 {
		t.Errorf("%d rows are left, want the 3 that have not expired", n)
	}
}

// The function that purges is for the application alone, on a database that has the role model and on one that
// has not: where nothing but the migration states who may run it, the migration must say that nobody but its
// owner may.
func TestNobodyButTheOwnerAndTheApplicationMayRunThePurge(t *testing.T) {
	t.Run("without the role model", func(t *testing.T) {
		_, pool := openStore(t)
		var public bool
		if err := pool.QueryRow(testCtx(t), `SELECT has_function_privilege('public', 'tenant.purge_expired_uploads()', 'EXECUTE')`).Scan(&public); err != nil {
			t.Fatal(dbtest.Describe(err))
		}
		if public {
			t.Errorf("PUBLIC may run tenant.purge_expired_uploads()")
		}
	})
	t.Run("with it", func(t *testing.T) {
		env := newRoleEnv(t)
		var public, dba bool
		if err := env.admin.QueryRow(testCtx(t), `
SELECT has_function_privilege('public', 'tenant.purge_expired_uploads()', 'EXECUTE'),
       has_function_privilege('anbud_dba', 'tenant.purge_expired_uploads()', 'EXECUTE')`).Scan(&public, &dba); err != nil {
			t.Fatal(dbtest.Describe(err))
		}
		if public || dba {
			t.Errorf("PUBLIC may run the purge: %v, the DBA group: %v", public, dba)
		}
	})
}

// What another role owns inside the schema is not the migrator's to change: PostgreSQL refuses a REVOKE on an
// object the caller holds nothing on, which would stop the migrator's every start. The migrator leaves it, and
// goes on with its own.
func TestAMigrationRunLeavesATableAnotherRoleOwnsInTheTenantSchemaAlone(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	for _, sql := range []string{
		`CREATE TABLE tenant.made_by_hand (x int)`,
		`GRANT SELECT ON tenant.made_by_hand TO anbud_app`,
		`CREATE FUNCTION tenant.made_by_hand_too() RETURNS int LANGUAGE sql AS 'SELECT 1'`,
		// A function is executable by PUBLIC when it is made, and the migrator would hold that much on it: it
		// holds nothing once that is taken away, which is when PostgreSQL refuses it a REVOKE.
		`REVOKE ALL ON FUNCTION tenant.made_by_hand_too() FROM PUBLIC`,
		`GRANT EXECUTE ON FUNCTION tenant.made_by_hand_too() TO anbud_app`,
	} {
		if _, err := env.admin.Exec(ctx, sql); err != nil {
			t.Fatalf("arrange %q: %s", sql, dbtest.Describe(err))
		}
	}
	migrator := dbtest.Pool(t, env.rs.MigratorDSN)
	if err := db.ApplyMigrations(ctx, migrator); err != nil {
		t.Fatalf("the migrator's start failed over what another role owns in its schema: %s", dbtest.Describe(err))
	}
	var table, routine bool
	if err := env.admin.QueryRow(ctx, `
SELECT has_table_privilege('anbud_app', 'tenant.made_by_hand', 'SELECT'),
       has_function_privilege('anbud_app', 'tenant.made_by_hand_too()', 'EXECUTE')`).Scan(&table, &routine); err != nil {
		t.Fatal(dbtest.Describe(err))
	}
	if !table || !routine {
		t.Errorf("what another role owns was changed: table %v, routine %v: it is that role's to grant", table, routine)
	}
	// And what the migrator owns is still taken back.
	if _, err := env.admin.Exec(ctx, `GRANT UPDATE ON tenant.tender_uploads TO anbud_app`); err != nil {
		t.Fatal(dbtest.Describe(err))
	}
	if err := db.ApplyMigrations(ctx, migrator); err != nil {
		t.Fatal(dbtest.Describe(err))
	}
	var update bool
	if err := env.admin.QueryRow(ctx, `SELECT has_table_privilege('anbud_app', 'tenant.tender_uploads', 'UPDATE')`).Scan(&update); err != nil {
		t.Fatal(dbtest.Describe(err))
	}
	if update {
		t.Errorf("a grant made by hand on the table the migrator owns was not taken back")
	}
}

func TestThePurgeFunctionIsPinnedAndOwnedByTheMigrator(t *testing.T) {
	env := newRoleEnv(t)
	var definer bool
	var config []string
	var owner string
	err := env.admin.QueryRow(testCtx(t), `
SELECT prosecdef, coalesce(proconfig, '{}'), pg_get_userbyid(proowner)
  FROM pg_proc WHERE oid = 'tenant.purge_expired_uploads()'::regprocedure`).Scan(&definer, &config, &owner)
	if err != nil {
		t.Fatalf("read the function: %s", dbtest.Describe(err))
	}
	if !definer || owner != dbtest.RoleMigrator {
		t.Errorf("security definer %v owner %q, want true and the migrator: the function must run as the table's owner", definer, owner)
	}
	if got := strings.Join(config, ","); got != "search_path=pg_catalog, pg_temp" {
		t.Errorf("function settings = %q, want search_path pinned to pg_catalog, pg_temp", got)
	}
	var publicMayRun bool
	if err := env.admin.QueryRow(testCtx(t), `SELECT has_function_privilege('public', 'tenant.purge_expired_uploads()', 'EXECUTE')`).Scan(&publicMayRun); err != nil || publicMayRun {
		t.Errorf("PUBLIC may run the purge: %v, %v", publicMayRun, err)
	}
}

// tenantColumns are what the application may insert: every column but id and created_at.
var tenantColumns = []string{
	"org_id", "notice_id", "uploaded_by", "archive_name", "archive_sha256", "archive_bytes", "documents",
	"findings", "read_count", "unread_count", "findings_dropped", "expires_at",
}

// The application's privileges on the schema are read back as they are: what it has, and what nobody
// else has.
func assertTenantPrivileges(t *testing.T, env roleEnv, after string) {
	t.Helper()
	ctx := testCtx(t)
	has := func(query string, args ...any) bool {
		var got bool
		if err := env.admin.QueryRow(ctx, query, args...).Scan(&got); err != nil {
			t.Fatalf("%s: %s", query, dbtest.Describe(err))
		}
		return got
	}
	for priv, want := range map[string]bool{"SELECT": true, "DELETE": true, "INSERT": false, "UPDATE": false, "TRUNCATE": false, "REFERENCES": false, "TRIGGER": false} {
		if got := has(`SELECT has_table_privilege('anbud_app', 'tenant.tender_uploads', $1)`, priv); got != want {
			t.Errorf("%s: anbud_app %s on the table = %v, want %v", after, priv, got, want)
		}
	}
	for _, col := range tenantColumns {
		if !has(`SELECT has_column_privilege('anbud_app', 'tenant.tender_uploads', $1, 'INSERT')`, col) {
			t.Errorf("%s: anbud_app may not insert %s", after, col)
		}
	}
	for _, col := range []string{"id", "created_at"} {
		for _, priv := range []string{"INSERT", "UPDATE"} {
			if has(`SELECT has_column_privilege('anbud_app', 'tenant.tender_uploads', $1, $2)`, col, priv) {
				t.Errorf("%s: anbud_app has %s on %s: the identity and the clock are the database's", after, priv, col)
			}
		}
	}
	for _, who := range []string{"public", "anbud_dba"} {
		if has(`SELECT has_any_column_privilege($1, 'tenant.tender_uploads', 'SELECT, INSERT, UPDATE, REFERENCES')`, who) ||
			has(`SELECT has_table_privilege($1, 'tenant.tender_uploads', 'SELECT')`, who) {
			t.Errorf("%s: %s has a privilege on the table", after, who)
		}
		if has(`SELECT has_schema_privilege($1, 'tenant', 'USAGE')`, who) {
			t.Errorf("%s: %s has USAGE on the schema", after, who)
		}
		if has(`SELECT has_function_privilege($1, 'tenant.purge_expired_uploads()', 'EXECUTE')`, who) {
			t.Errorf("%s: %s may run the purge", after, who)
		}
	}
	if !has(`SELECT has_schema_privilege('anbud_app', 'tenant', 'USAGE') AND has_function_privilege('anbud_app', 'tenant.purge_expired_uploads()', 'EXECUTE')`) {
		t.Errorf("%s: anbud_app lost USAGE on the schema or the purge", after)
	}
}

// tenant_privileges.sql runs after every migration run. A grant a person added by hand, to the
// application, to PUBLIC or to the DBA group, is taken back at the next start, and one that is missing is
// given.
func TestThePrivilegesOnTheTenantSchemaAreStatedAgainAtEveryStart(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	assertTenantPrivileges(t, env, "after the first migration")

	for _, sql := range []string{
		`GRANT UPDATE, TRUNCATE ON tenant.tender_uploads TO anbud_app`,
		`REVOKE DELETE ON tenant.tender_uploads FROM anbud_app`,
		`GRANT INSERT (id, created_at) ON tenant.tender_uploads TO anbud_app`,
		`GRANT SELECT ON tenant.tender_uploads TO PUBLIC`,
		`GRANT SELECT ON tenant.tender_uploads TO anbud_dba`,
		`GRANT USAGE ON SCHEMA tenant TO PUBLIC`,
		`GRANT USAGE ON SCHEMA tenant TO anbud_dba`,
		`GRANT EXECUTE ON FUNCTION tenant.purge_expired_uploads() TO PUBLIC`,
		`GRANT EXECUTE ON FUNCTION tenant.purge_expired_uploads() TO anbud_dba`,
	} {
		if _, err := env.admin.Exec(ctx, sql); err != nil {
			t.Fatalf("arrange %q: %s", sql, dbtest.Describe(err))
		}
	}
	if got := outcome(ctx, env.app, `UPDATE tenant.tender_uploads SET read_count = read_count`); got == "42501" {
		t.Fatalf("the arrangement did not give UPDATE")
	}

	migrator := dbtest.Pool(t, env.rs.MigratorDSN)
	if err := db.ApplyMigrations(ctx, migrator); err != nil {
		t.Fatalf("ApplyMigrations again: %s", dbtest.Describe(err))
	}
	assertTenantPrivileges(t, env, "after a start")
}

// The application may not choose the identity of a row, which could squat a future id and make a later
// insert collide with it, nor its creation time, which would carry the expiry the table bounds past its
// bound. Both are refused for lack of privilege, whatever the statement says.
func TestTheAppRoleCannotChooseTheIdentityOrTheClock(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	for name, sql := range map[string]string{
		"an identity of its own": `
INSERT INTO tenant.tender_uploads (id, org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, expires_at)
OVERRIDING SYSTEM VALUE VALUES (999999, 'org-a', '2026-100001', repeat('a', 64), 1, '[]', '[]', 0, 0, now() + interval '1 day')`,
		"a creation time of its own": `
INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, created_at, expires_at)
VALUES ('org-a', '2026-100001', repeat('a', 64), 1, '[]', '[]', 0, 0, now() + interval '50 years', now() + interval '51 years')`,
	} {
		if got := outcome(ctx, env.app, sql); got != "42501" {
			t.Errorf("%s as the app role: %s, want 42501 (insufficient privilege)", name, got)
		}
	}
	if n := count(t, env.admin, `SELECT count(*) FROM tenant.tender_uploads`); n != 0 {
		t.Errorf("rows = %d: a refused insert left a row", n)
	}
	// The store itself, which names neither column, is unaffected.
	if _, _, err := env.store.Create(ctx, sample("org-a", "2026-100001", "a")); err != nil {
		t.Errorf("Create as the app role: %s", dbtest.Describe(err))
	}
}

// The two-year bound is in hours so that a retention the service may ask for (730 days) passes in a
// session whose clock changes, and in a leap year; and what is over the bound is refused, by that check
// and not by another.
func TestTheTwoYearBoundHoldsWhateverTheTimeZone(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	conn, err := env.admin.Acquire(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Release()
	if _, err := conn.Exec(ctx, `SET TIME ZONE 'Europe/Oslo'`); err != nil {
		t.Fatal(err)
	}
	// Created on 30 March 2024, twelve noon, the day before the clock went forward in Oslo.
	insert := func(tag string, hours int) error {
		_, err := conn.Exec(ctx, `
INSERT INTO tenant.tender_uploads (org_id, notice_id, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, created_at, expires_at)
VALUES ('org-a', '2026-100001', repeat('a', 63) || $1::text, 1, '[]', '[]', 0, 0, timestamptz '2024-03-30 12:00:00+01', timestamptz '2024-03-30 12:00:00+01' + make_interval(hours => $2))`,
			tag, hours)
		return err
	}
	if err := insert("a", 730*24); err != nil {
		t.Errorf("a retention of 730 days: %s, want it accepted in any zone", dbtest.Describe(err))
	}
	if err := insert("b", 732*24); err != nil {
		t.Errorf("a retention of 732 days, the bound itself: %s", dbtest.Describe(err))
	}
	err = insert("c", 732*24+1)
	var pgErr *pgconn.PgError
	if !errors.As(err, &pgErr) || pgErr.ConstraintName != "tender_uploads_expires_within_two_years" {
		t.Errorf("a retention of 732 days and an hour: %v, want the two-year check to refuse it", err)
	}
}

// A schema that another role made (a service run with a superuser DSN before roles.sql took it over) must
// not stop the migrator's start with a permission error: it is left for roles.sql to take over.
func TestAMigrationRunLeavesATenantSchemaAnotherRoleOwnsAlone(t *testing.T) {
	env := newRoleEnv(t)
	ctx := testCtx(t)
	var admin string
	if err := env.admin.QueryRow(ctx, `SELECT current_user`).Scan(&admin); err != nil {
		t.Fatal(err)
	}
	for _, sql := range []string{
		`ALTER FUNCTION tenant.purge_expired_uploads() OWNER TO ` + pgIdent(admin),
		`ALTER TABLE tenant.tender_uploads OWNER TO ` + pgIdent(admin),
		`ALTER SCHEMA tenant OWNER TO ` + pgIdent(admin),
	} {
		if _, err := env.admin.Exec(ctx, sql); err != nil {
			t.Fatalf("arrange %q: %s", sql, dbtest.Describe(err))
		}
	}
	migrator := dbtest.Pool(t, env.rs.MigratorDSN)
	if err := db.ApplyMigrations(ctx, migrator); err != nil {
		t.Errorf("the migrator's start failed over a schema it does not own: %s", dbtest.Describe(err))
	}
}

func pgIdent(name string) string { return `"` + strings.ReplaceAll(name, `"`, `""`) + `"` }

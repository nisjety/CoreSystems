package db_test

import (
	"slices"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// How a database gets to the role model, and how it stays there. Needs a throwaway
// server: run with make test-db.

// Today one role, in practice the server's superuser, migrates and owns everything.
// scripts/roles.sql run on such a database moves every table, sequence and function
// to the migrator, keeps the data, and gives the DBA group its read access at once.
// The application's table privileges come with the migrator's next run, which is the
// first start of doffin-sync with ANBUD_MIGRATE_DSN set: until then the role has none.
func TestRolesScriptTakesOverADatabaseMigratedWithOneRole(t *testing.T) {
	dsn := dbtest.Database(t) // migrated by the server's own role, as doffin-sync does today
	admin := dbtest.Pool(t, dsn)
	ctx := roleCtx(t)

	owners := scanStrings(ctx, t, admin, `SELECT DISTINCT tableowner FROM pg_tables WHERE schemaname = 'public'`)
	if len(owners) != 1 || owners[0] == dbtest.RoleMigrator {
		t.Fatalf("before the script the tables are owned by %v, want the server's role alone", owners)
	}
	var runs int
	if _, err := admin.Exec(ctx, `INSERT INTO sync_runs (mode, range_from, range_to) VALUES ('sync', '2024-01-02', '2024-01-02')`); err != nil {
		t.Fatalf("seed a run: %s", dbtest.Describe(err))
	}

	rs := dbtest.RolesOn(t, dsn)

	if got := scanStrings(ctx, t, admin, `SELECT DISTINCT tableowner FROM pg_tables WHERE schemaname = 'public'`); !slices.Equal(got, []string{dbtest.RoleMigrator}) {
		t.Errorf("after the script the tables are owned by %v, want only %s", got, dbtest.RoleMigrator)
	}
	if got := scanStrings(ctx, t, admin, `SELECT DISTINCT pg_get_userbyid(p.proowner) FROM pg_proc p WHERE p.pronamespace = 'public'::regnamespace`); !slices.Equal(got, []string{dbtest.RoleMigrator}) {
		t.Errorf("after the script the functions are owned by %v, want only %s", got, dbtest.RoleMigrator)
	}
	if got := scanStrings(ctx, t, admin, `SELECT DISTINCT pg_get_userbyid(c.relowner) FROM pg_class c WHERE c.relkind = 'S' AND c.relnamespace = 'public'::regnamespace`); !slices.Equal(got, []string{dbtest.RoleMigrator}) {
		t.Errorf("after the script the sequences are owned by %v, want only %s", got, dbtest.RoleMigrator)
	}
	if err := admin.QueryRow(ctx, `SELECT count(*) FROM sync_runs`).Scan(&runs); err != nil || runs != 1 {
		t.Errorf("the seeded run is gone after the script: %d rows (error %s)", runs, dbtest.Describe(err))
	}
	for _, table := range allTables {
		if scanBool(ctx, t, admin, `SELECT has_table_privilege('anbud_app', $1, 'SELECT')`, "public."+table) {
			t.Errorf("the app role can read %s before the migrator has run: the script grants no table privilege to it", table)
		}
		if !scanBool(ctx, t, admin, `SELECT has_table_privilege('anbud_dba', $1, 'SELECT')`, "public."+table) {
			t.Errorf("the DBA group cannot read %s after the script", table)
		}
	}

	// The migrator's run has nothing to apply, and gives the application its privileges.
	// The matrix is the role model's, the application's SELECT on the two relations that
	// migration 0006 added before the roles existed included: privileges.sql names them.
	migrator := dbtest.Pool(t, rs.MigratorDSN)
	if err := db.ApplyMigrations(ctx, migrator); err != nil {
		t.Fatalf("ApplyMigrations as the migrator on the taken-over database: %s", dbtest.Describe(err))
	}
	if got, want := grantedMatrix(ctx, t, admin), expectedMatrix(); !equalMatrix(got, want) {
		t.Errorf("privilege matrix after the migrator's run =\n%s\nwant\n%s", formatMatrix(got), formatMatrix(want))
	}
	app := dbtest.Pool(t, rs.AppDSN)
	seedAsApp(ctx, t, app)
}

// Running the script again is also how a change someone made by hand is undone: an
// attribute switched on, a role that was given a login, CREATE on the schema, and write
// privileges on tables for the DBA group. Afterwards every ACL, owner, default and role
// setting is what it was before the changes.
func TestRolesScriptCorrectsDriftInRolesAndSchemaGrants(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	before := aclSnapshot(ctx, t, admin)

	for _, sql := range []string{
		`ALTER ROLE anbud_app SUPERUSER CREATEDB CREATEROLE BYPASSRLS REPLICATION`,
		`ALTER ROLE anbud_migrator SUPERUSER`,
		`ALTER ROLE anbud_dba LOGIN`,
		`ALTER ROLE anbud_app RESET search_path`,
		`GRANT CREATE ON SCHEMA public TO anbud_app, anbud_dba`,
		`GRANT INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER ON sync_runs, raw_documents, raw_redactions TO anbud_dba`,
	} {
		if _, err := admin.Exec(ctx, sql); err != nil {
			t.Fatalf("arrange %q: %s", sql, dbtest.Describe(err))
		}
	}
	if slices.Equal(before, aclSnapshot(ctx, t, admin)) {
		t.Fatal("the arranged drift is not visible in the snapshot; the test would prove nothing")
	}

	dbtest.RunRolesScript(t, rs)

	if after := aclSnapshot(ctx, t, admin); !slices.Equal(before, after) {
		t.Errorf("the script did not undo the drift:\nbefore:\n%s\nafter:\n%s", strings.Join(before, "\n"), strings.Join(after, "\n"))
	}
	if got, want := grantedMatrix(ctx, t, admin), expectedMatrix(); !equalMatrix(got, want) {
		t.Errorf("privilege matrix after the script =\n%s\nwant\n%s", formatMatrix(got), formatMatrix(want))
	}
}

// Every migration run states the application's table privileges again, so a grant
// someone made by hand, or one a role lost, is corrected at the next start. The run
// leaves alone the tables a later migration adds.
func TestApplyMigrationsReassertsTheAppPrivileges(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	migrator := dbtest.Pool(t, rs.MigratorDSN)

	for _, sql := range []string{
		`GRANT DELETE, TRUNCATE, REFERENCES, TRIGGER, UPDATE ON raw_documents, raw_search_pages, raw_redactions, raw_documents_live TO anbud_app`,
		`GRANT DELETE, TRUNCATE ON sync_runs, sync_days TO anbud_app`,
		`GRANT UPDATE ON download_attempts TO anbud_app`,
		`GRANT ALL ON schema_migrations TO anbud_app`,
		`REVOKE UPDATE ON sync_days FROM anbud_app`,
		`REVOKE INSERT ON raw_search_pages FROM anbud_app`,
	} {
		if _, err := admin.Exec(ctx, sql); err != nil {
			t.Fatalf("arrange %q: %s", sql, dbtest.Describe(err))
		}
	}
	if equalMatrix(grantedMatrix(ctx, t, admin), expectedMatrix()) {
		t.Fatal("the arranged drift is not visible in the matrix; the test would prove nothing")
	}

	if err := db.ApplyMigrations(ctx, migrator); err != nil {
		t.Fatalf("ApplyMigrations as the migrator: %s", dbtest.Describe(err))
	}
	if got, want := grantedMatrix(ctx, t, admin), expectedMatrix(); !equalMatrix(got, want) {
		t.Errorf("privilege matrix after the run =\n%s\nwant\n%s", formatMatrix(got), formatMatrix(want))
	}
}

// The matrix is read with has_table_privilege, which cannot see a grant on a single column, and a
// column grant is as good as a table grant for the one column: GRANT UPDATE (content_type) ON
// raw_documents would let the application rewrite a body's type. The runner takes it back with the
// REVOKE it already issues, because a REVOKE on a table also takes the privilege from every
// column of it, and the script does the same for the DBA group. Each is read back with
// has_any_column_privilege, in the application's case after the next run of the runner and in the
// group's after the next run of the script.
func TestColumnPrivilegesGrantedByHandAreTakenBack(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	migrator := dbtest.Pool(t, rs.MigratorDSN)

	if got := columnOnlyPrivileges(ctx, t, admin); len(got) != 0 {
		t.Fatalf("before the arrangement privileges are held on columns only: %v", got)
	}
	for _, sql := range []string{
		`GRANT UPDATE (content_type) ON raw_documents TO anbud_app`,
		`GRANT UPDATE (content_type), REFERENCES (id) ON raw_search_pages TO anbud_app`,
		`GRANT UPDATE (message), REFERENCES (id) ON download_attempts TO anbud_app`,
		`GRANT INSERT (reason), UPDATE (reason) ON raw_redactions TO anbud_app`,
		`GRANT INSERT (body) ON raw_documents_live TO anbud_app`,
		`GRANT SELECT (version), UPDATE (name) ON schema_migrations TO anbud_app`,
		`GRANT UPDATE (content_type), INSERT (content_type), REFERENCES (id) ON raw_documents TO anbud_dba`,
		`GRANT UPDATE (last_error) ON sync_runs TO anbud_dba`,
	} {
		if _, err := admin.Exec(ctx, sql); err != nil {
			t.Fatalf("arrange %q: %s", sql, dbtest.Describe(err))
		}
	}
	// The drift is invisible to the table-level matrix, which is why it needs this test, and
	// visible to the column-level read.
	if !equalMatrix(grantedMatrix(ctx, t, admin), expectedMatrix()) {
		t.Fatal("the arranged column grants changed the table-level matrix; the test is meant to show what that matrix cannot see")
	}
	if got := columnOnlyPrivileges(ctx, t, admin); len(got) < 10 {
		t.Fatalf("the arranged column grants are not visible to has_any_column_privilege (%v); the test would prove nothing", got)
	}

	if err := db.ApplyMigrations(ctx, migrator); err != nil {
		t.Fatalf("ApplyMigrations as the migrator: %s", dbtest.Describe(err))
	}
	for _, got := range columnOnlyPrivileges(ctx, t, admin) {
		if !strings.HasPrefix(got, dbtest.RoleDBA+" ") {
			t.Errorf("the runner left a privilege held on a column only: %s", got)
		}
	}
	if got := columnOnlyPrivileges(ctx, t, admin); len(got) == 0 {
		t.Error("the runner took back the DBA group's column grants as well; it is the script's to do, and the test would not tell the two apart")
	}

	dbtest.RunRolesScript(t, rs)
	if got := columnOnlyPrivileges(ctx, t, admin); len(got) != 0 {
		t.Errorf("privileges are still held on columns only after the runner and the script: %v", got)
	}
	if got, want := grantedMatrix(ctx, t, admin), expectedMatrix(); !equalMatrix(got, want) {
		t.Errorf("privilege matrix after both =\n%s\nwant\n%s", formatMatrix(got), formatMatrix(want))
	}
}

// Tables, sequences and functions that later migrations add are covered by the default
// privileges for the migrator's objects: the application reads and inserts and
// never updates, the DBA group reads, and a function is not executable by anyone
// the migration does not name. A later run of the runner leaves them as they are.
func TestDefaultPrivilegesCoverWhatLaterMigrationsAdd(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	migrator := dbtest.Pool(t, rs.MigratorDSN)
	app := dbtest.Pool(t, rs.AppDSN)

	for _, sql := range []string{
		`CREATE TABLE later_ident (id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, v int NOT NULL DEFAULT 0)`,
		`CREATE TABLE later_serial (id serial PRIMARY KEY, v int NOT NULL DEFAULT 0)`,
		`CREATE FUNCTION later_fn() RETURNS int LANGUAGE sql AS 'SELECT 1'`,
		`CREATE FUNCTION later_bump() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN NEW.v := NEW.v + 1; RETURN NEW; END $$`,
		`CREATE TRIGGER later_ident_bump BEFORE INSERT ON later_ident FOR EACH ROW EXECUTE FUNCTION later_bump()`,
	} {
		if _, err := migrator.Exec(ctx, sql); err != nil {
			t.Fatalf("a later migration, as the migrator: %q: %s", sql, dbtest.Describe(err))
		}
	}

	check := func(stage string) {
		t.Helper()
		for _, c := range []struct {
			role, priv string
			want       bool
		}{
			{dbtest.RoleApp, "SELECT", true},
			{dbtest.RoleApp, "INSERT", true},
			{dbtest.RoleApp, "UPDATE", false},
			{dbtest.RoleApp, "DELETE", false},
			{dbtest.RoleApp, "TRUNCATE", false},
			{dbtest.RoleApp, "TRIGGER", false},
			{dbtest.RoleApp, "REFERENCES", false},
			{dbtest.RoleDBA, "SELECT", true},
			{dbtest.RoleDBA, "INSERT", false},
			{dbtest.RoleDBA, "UPDATE", false},
			{dbtest.RoleDBA, "DELETE", false},
			{dbtest.RoleDBA, "TRUNCATE", false},
		} {
			if got := scanBool(ctx, t, admin, `SELECT has_table_privilege($1, 'public.later_ident', $2)`, c.role, c.priv); got != c.want {
				t.Errorf("%s: %s %s on a table a later migration added = %v, want %v", stage, c.role, c.priv, got, c.want)
			}
		}
		for _, role := range []string{dbtest.RoleApp, dbtest.RoleDBA} {
			if scanBool(ctx, t, admin, `SELECT has_function_privilege($1, 'public.later_fn()', 'EXECUTE')`, role) {
				t.Errorf("%s: %s can execute a function a later migration added; PUBLIC must not hold EXECUTE by default", stage, role)
			}
		}
	}
	check("after the later migration")

	// An identity column needs no sequence privilege. A serial column does, which
	// is why a migration that adds a table uses identity columns.
	if got := outcome(ctx, app, `INSERT INTO later_ident (v) VALUES (1)`); got != stateOK {
		t.Errorf("an insert into a later table with an identity column = %s, want ok", got)
	}
	// A trigger fires for the role that writes the table, though PUBLIC has no EXECUTE on
	// its function: the trigger turned the 1 into a 2.
	var bumped int
	if err := admin.QueryRow(ctx, `SELECT v FROM later_ident`).Scan(&bumped); err != nil || bumped != 2 {
		t.Errorf("the trigger on a later table left v = %d (error %s), want 2: it must fire without EXECUTE", bumped, dbtest.Describe(err))
	}
	for _, role := range []string{dbtest.RoleApp, dbtest.RoleDBA} {
		if scanBool(ctx, t, admin, `SELECT has_function_privilege($1, 'public.later_bump()', 'EXECUTE')`, role) {
			t.Errorf("%s can execute a trigger function a later migration added", role)
		}
	}
	if got := outcome(ctx, app, `INSERT INTO later_serial (v) VALUES (1)`); got != stateDenied {
		t.Errorf("an insert into a later table with a serial column = %s, want %s: a serial column needs a grant on its sequence", got, stateDenied)
	}
	if got := outcome(ctx, app, `SELECT later_fn()`); got != stateDenied {
		t.Errorf("a call of a function a later migration added = %s, want %s", got, stateDenied)
	}
	for _, sql := range []string{`UPDATE later_ident SET v = 2`, `DELETE FROM later_ident`, `TRUNCATE later_ident`} {
		if got := outcome(ctx, app, sql); got != stateDenied {
			t.Errorf("%q as the app role = %s, want %s", sql, got, stateDenied)
		}
	}

	if err := db.ApplyMigrations(ctx, migrator); err != nil {
		t.Fatalf("ApplyMigrations as the migrator: %s", dbtest.Describe(err))
	}
	check("after another run of the runner")
}

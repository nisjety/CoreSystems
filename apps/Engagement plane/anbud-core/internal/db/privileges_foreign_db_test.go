package db_test

import (
	"testing"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// A relation in public that the migrator neither owns nor holds a privilege on, such as a
// view an extension installed or a table a superuser made by hand, must not stop the
// migration runner. PostgreSQL refuses a REVOKE on a relation the caller has no privilege
// on, and a REVOKE ... ON ALL TABLES IN SCHEMA names every relation of the schema, so one
// such object would roll the whole migration transaction back at every start. The runner
// restates privileges only on relations it may change. Needs a throwaway server: run with
// make test-db.
func TestApplyMigrationsIgnoresRelationsOfOtherOwners(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	migrator := dbtest.Pool(t, rs.MigratorDSN)

	// The server's own role, a superuser, creates them: the migrator owns neither and,
	// with the privileges revoked from PUBLIC, holds nothing on either.
	for _, sql := range []string{
		`CREATE TABLE public.other_owner_table (id int)`,
		`CREATE VIEW public.other_owner_view AS SELECT 1 AS one`,
		`REVOKE ALL ON public.other_owner_table, public.other_owner_view FROM PUBLIC`,
	} {
		if _, err := admin.Exec(ctx, sql); err != nil {
			t.Fatalf("arrange %q: %s", sql, dbtest.Describe(err))
		}
	}
	for _, rel := range []string{"public.other_owner_table", "public.other_owner_view"} {
		if scanBool(ctx, t, admin, `SELECT pg_has_role('anbud_migrator', c.relowner, 'USAGE') FROM pg_class c WHERE c.oid = $1::regclass`, rel) {
			t.Fatalf("%s is owned by the migrator or a role it holds; the test would prove nothing", rel)
		}
		if scanBool(ctx, t, admin, `SELECT has_table_privilege('anbud_migrator', $1, 'SELECT')`, rel) {
			t.Fatalf("the migrator can read %s; the test would prove nothing", rel)
		}
	}

	if err := db.ApplyMigrations(ctx, migrator); err != nil {
		t.Fatalf("ApplyMigrations as the migrator with relations of another owner in public: %s", dbtest.Describe(err))
	}
	if got, want := grantedMatrix(ctx, t, admin), expectedMatrix(); !equalMatrix(got, want) {
		t.Errorf("privilege matrix after the run =\n%s\nwant\n%s", formatMatrix(got), formatMatrix(want))
	}
	// The relations of other owners are left as they were: the runner gave the
	// application nothing on them either.
	for _, rel := range []string{"public.other_owner_table", "public.other_owner_view"} {
		if scanBool(ctx, t, admin, `SELECT has_table_privilege('anbud_app', $1, 'SELECT')`, rel) {
			t.Errorf("the app role can read %s, which the runner does not own", rel)
		}
	}
}

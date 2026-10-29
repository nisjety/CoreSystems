package db_test

import (
	"slices"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// Who may run the redaction function is decided by its ACL and by the DBA group's members, and
// the roles script keeps it that way: the owner and the DBA group hold EXECUTE, nobody else
// holds it directly, and the group cannot pass it on. A grant that a DBA made by hand to any
// other role, in a hurry, is taken back by the next run, whoever the role is, with
// everything that role passed on. Needs a throwaway server: run with make test-db.

const redactionFn = `public.anbud_redact_raw_document(bigint, text, text)`

func TestRolesScriptRevokesEveryOtherDirectGrantOfTheRedactionFunction(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	rs := env.rs

	alice := "anbud_test_acl_alice"
	bob := "anbud_test_acl_bob"
	carol := "anbud_test_acl_carol"
	erin := "anbud_test_acl_erin"
	dbtest.CreateLoginRole(t, rs, alice, "")
	bobPool := dbtest.Pool(t, dbtest.CreateLoginRole(t, rs, bob, ""))
	dbtest.CreateLoginRole(t, rs, carol, "")
	dbtest.CreateLoginRole(t, rs, erin, "")

	// What a hurried DBA might have done: a plain grant to a role, a grant with the grant
	// option that the role then used, and the group's own grant option, which a member of the
	// group used to hand the function to somebody who is in no group.
	for _, sql := range []string{
		`GRANT EXECUTE ON FUNCTION ` + redactionFn + ` TO ` + alice,
		`GRANT EXECUTE ON FUNCTION ` + redactionFn + ` TO ` + bob + ` WITH GRANT OPTION`,
		`GRANT EXECUTE ON FUNCTION ` + redactionFn + ` TO anbud_dba WITH GRANT OPTION`,
	} {
		if _, err := env.admin.Exec(ctx, sql); err != nil {
			t.Fatalf("arrange %q: %s", sql, dbtest.Describe(err))
		}
	}
	if _, err := bobPool.Exec(ctx, `GRANT EXECUTE ON FUNCTION `+redactionFn+` TO `+carol); err != nil {
		t.Fatalf("the role with the grant option cannot pass it on: %s", dbtest.Describe(err))
	}
	if _, err := env.dba.Exec(ctx, `GRANT EXECUTE ON FUNCTION `+redactionFn+` TO `+erin); err != nil {
		t.Fatalf("a member of the group cannot use the group's grant option: %s", dbtest.Describe(err))
	}
	for _, role := range []string{alice, bob, carol, erin} {
		if !scanBool(ctx, t, env.admin, `SELECT has_function_privilege($1, $2, 'EXECUTE')`, role, redactionRegproc) {
			t.Fatalf("control: %s cannot execute the function before the script, so the test would prove nothing", role)
		}
	}
	snapshot := aclSnapshot(ctx, t, env.admin)

	dbtest.RunRolesScript(t, rs)

	if got, want := functionGrants(ctx, t, env.admin), []string{"anbud_dba EXECUTE", "anbud_migrator EXECUTE"}; !slices.Equal(got, want) {
		t.Errorf("the function's ACL after the script = %v, want %v", got, want)
	}
	if scanBool(ctx, t, env.admin, `SELECT EXISTS (SELECT 1 FROM pg_proc p, aclexplode(p.proacl) a WHERE p.oid = $1::regprocedure AND a.is_grantable)`, redactionRegproc) {
		t.Error("someone still holds EXECUTE on the function with the grant option")
	}
	for _, role := range []string{alice, bob, carol, erin, dbtest.RoleApp} {
		if scanBool(ctx, t, env.admin, `SELECT has_function_privilege($1, $2, 'EXECUTE')`, role, redactionRegproc) {
			t.Errorf("%s can still execute the redaction function after the script", role)
		}
	}
	// What the group's members hold, they hold through the group, and still do.
	var tomb int64
	id := env.seedNotice(ctx, t, "2026-200003", contactBody(""))
	if err := env.dba.QueryRow(ctx, redactSQL, id, "objection", "T-3").Scan(&tomb); err != nil {
		t.Errorf("a member of anbud_dba cannot run the function after the script: %s", dbtest.Describe(err))
	}
	// The script changes nothing it did not have to: every ACL but the function's is as it was.
	withoutFunction := func(rows []string) []string {
		return slices.DeleteFunc(slices.Clone(rows), func(r string) bool { return strings.HasPrefix(r, "fn anbud_redact_raw_document ") })
	}
	if before, after := withoutFunction(snapshot), withoutFunction(aclSnapshot(ctx, t, env.admin)); !slices.Equal(before, after) {
		t.Errorf("the script changed an ACL other than the redaction function's:\nbefore:\n%s\nafter:\n%s", strings.Join(before, "\n"), strings.Join(after, "\n"))
	}
}

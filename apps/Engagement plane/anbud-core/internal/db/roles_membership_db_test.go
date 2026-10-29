package db_test

import (
	"context"
	"slices"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// The three roles are members of nothing. A membership of anbud_app in anbud_migrator makes it
// the owner, in anbud_dba it can erase a notice body under its own login, in
// pg_execute_server_program it can run programs, and the old runtime check saw none of it, and
// scripts/roles.sql did not undo any of it. Needs a throwaway server: run with make test-db.

// membershipsOfTheThree lists every membership that one of the three roles holds, as
// "group <- member", in a fixed order. Rows of other members, such as a human DBA's, are left out.
func membershipsOfTheThree(ctx context.Context, t *testing.T, admin *pgxpool.Pool) []string {
	t.Helper()
	return scanStrings(ctx, t, admin, `
		SELECT pg_get_userbyid(am.roleid) || ' <- ' || pg_get_userbyid(am.member)
		  FROM pg_auth_members am
		 WHERE pg_get_userbyid(am.member) IN ('anbud_migrator', 'anbud_app', 'anbud_dba')
		 ORDER BY 1`)
}

// Whatever was granted to the three roles by hand, the next run of the script takes it back:
// the memberships of the application, the owner and the DBA group, in other roles and in each
// other. The members of the DBA group stay members. Afterwards the application cannot run the
// redaction function, which it could while it was a member of the group, and the check sees a role
// with nothing to answer for.
func TestRolesScriptTakesBackEveryMembershipOfTheThreeRoles(t *testing.T) {
	rs, admin := migratedRoles(t)
	env := redactionEnvOn(t, rs, admin) // env.dba is a login role in anbud_dba: a human DBA
	ctx := roleCtx(t)
	first := env.seedNotice(ctx, t, "2026-200001", contactBody(""))
	second := env.seedNotice(ctx, t, "2026-200002", contactBody("<Second/>"))
	snapshot := aclSnapshot(ctx, t, admin)

	if got := membershipsOfTheThree(ctx, t, admin); len(got) != 0 {
		t.Fatalf("before the arrangement the three roles are members of %v, want none", got)
	}
	for _, sql := range []string{
		`GRANT anbud_migrator TO anbud_app`,
		`GRANT pg_execute_server_program TO anbud_app`,
		`GRANT anbud_dba TO anbud_app`,
		`GRANT anbud_dba TO anbud_migrator`,
		`GRANT pg_signal_backend TO anbud_migrator`,
		`GRANT pg_monitor TO anbud_dba`,
	} {
		if _, err := admin.Exec(ctx, sql); err != nil {
			t.Fatalf("arrange %q: %s", sql, dbtest.Describe(err))
		}
	}
	if got := membershipsOfTheThree(ctx, t, admin); len(got) != 6 {
		t.Fatalf("the arrangement left the three roles members of %v, want 6 memberships", got)
	}

	// The harm, which the old script left in place: through the DBA group the application
	// runs the redaction function, erases a body and records the tombstone under its own login.
	var tomb int64
	if err := env.app.QueryRow(ctx, redactSQL, first, "erasure_request", "T-1").Scan(&tomb); err != nil {
		t.Fatalf("control: the application, a member of anbud_dba, cannot run the redaction function: %s", dbtest.Describe(err))
	}
	if by := scanStrings(ctx, t, admin, `SELECT redacted_by FROM raw_redactions WHERE id = $1`, tomb); !slices.Equal(by, []string{dbtest.RoleApp}) {
		t.Fatalf("control: the tombstone records %v, want the application's own login", by)
	}
	facts, err := db.ReadRoleFacts(ctx, env.app)
	if err != nil || !facts.HoldsDBARole || !facts.HoldsMigratorRole || !facts.HoldsPredefinedRole {
		t.Fatalf("control: the check reads %+v (error %s) for the application role, want it to see the three memberships", facts, dbtest.Describe(err))
	}

	dbtest.RunRolesScript(t, rs)

	if got := membershipsOfTheThree(ctx, t, admin); len(got) != 0 {
		t.Errorf("after the script the three roles are still members of %v, want none", got)
	}
	if got := scanStrings(ctx, t, admin, `SELECT pg_get_userbyid(am.member) FROM pg_auth_members am WHERE pg_get_userbyid(am.roleid) = 'anbud_dba' ORDER BY 1`); !slices.Equal(got, []string{dbaLogin}) {
		t.Errorf("the members of anbud_dba are %v after the script, want the human DBA %s alone", got, dbaLogin)
	}
	if err := env.app.QueryRow(ctx, redactSQL, second, "erasure_request", "T-2").Scan(&tomb); dbtest.Outcome(err) != stateDenied {
		t.Errorf("the application runs the redaction function after the script = %s, want %s", dbtest.Outcome(err), stateDenied)
	}
	if doc := readDoc(ctx, t, admin, second); doc.RedactedAt != nil || len(doc.Body) == 0 {
		t.Error("the application erased a body after the script")
	}
	if got := scanInt(ctx, t, admin, `SELECT count(*) FROM raw_redactions`); got != 1 {
		t.Errorf("there are %d tombstones after the script, want only the control's", got)
	}
	if facts, err := db.ReadRoleFacts(ctx, env.app); err != nil || facts.Excessive() {
		t.Errorf("the check still reads %+v (error %s) for the application role, want no fact", facts, dbtest.Describe(err))
	}
	if after := aclSnapshot(ctx, t, admin); !slices.Equal(snapshot, after) {
		t.Errorf("the script left the ACLs different from before the arrangement:\nbefore:\n%s\nafter:\n%s", strings.Join(snapshot, "\n"), strings.Join(after, "\n"))
	}
}

// On PostgreSQL 16 a membership records who granted it, and a REVOKE removes only the grants
// of the role that issues it, a superuser counting as the bootstrap superuser. A membership that
// a role with CREATEROLE granted, through the admin option it held, is therefore not removed
// by a plain REVOKE from a superuser: the script revokes each grant by its own grantor.
func TestRolesScriptTakesBackAMembershipSomeoneElseGranted(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	if v := serverVersion(ctx, t, admin); v < 160000 {
		t.Skip("a membership records its grantor from PostgreSQL 16 on")
	}

	granter := dbtest.Pool(t, dbtest.CreateLoginRole(t, rs, "anbud_test_granter", "CREATEROLE"))
	if _, err := admin.Exec(ctx, `GRANT anbud_migrator TO anbud_test_granter WITH ADMIN OPTION`); err != nil {
		t.Fatalf("arrange the admin option: %s", dbtest.Describe(err))
	}
	if _, err := granter.Exec(ctx, `GRANT anbud_migrator TO anbud_app`); err != nil {
		t.Fatalf("the role with the admin option cannot grant the membership: %s", dbtest.Describe(err))
	}
	grantors := scanStrings(ctx, t, admin, `SELECT pg_get_userbyid(grantor) FROM pg_auth_members WHERE roleid = 'anbud_migrator'::regrole AND member = 'anbud_app'::regrole`)
	if !slices.Equal(grantors, []string{"anbud_test_granter"}) {
		t.Fatalf("the membership was granted by %v, want the role with the admin option; the test would prove nothing", grantors)
	}

	dbtest.RunRolesScript(t, rs)

	if got := membershipsOfTheThree(ctx, t, admin); len(got) != 0 {
		t.Errorf("after the script the three roles are still members of %v, want none", got)
	}
	if facts, err := db.ReadRoleFacts(ctx, dbtest.Pool(t, rs.AppDSN)); err != nil || facts.Excessive() {
		t.Errorf("the check reads %+v (error %s) for the application role after the script, want no fact", facts, dbtest.Describe(err))
	}
}

// The three roles are members of nothing, and the script takes a membership back even when it
// was granted with the admin option and passed on: nothing that depends on it is left either.
func TestRolesScriptTakesBackAMembershipThatWasPassedOn(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)

	// The application holds the owner role with the admin option, and gives it to a role of its
	// own (it needs no privilege for that, only the option it holds).
	if _, err := admin.Exec(ctx, `GRANT anbud_migrator TO anbud_app WITH ADMIN OPTION`); err != nil {
		t.Fatalf("arrange: %s", dbtest.Describe(err))
	}
	dbtest.CreateLoginRole(t, rs, "anbud_test_passed_on", "")
	if _, err := dbtest.Pool(t, rs.AppDSN).Exec(ctx, `GRANT anbud_migrator TO anbud_test_passed_on`); err != nil {
		t.Fatalf("the application cannot pass the owner role on: %s", dbtest.Describe(err))
	}

	dbtest.RunRolesScript(t, rs)

	if got := membershipsOfTheThree(ctx, t, admin); len(got) != 0 {
		t.Errorf("after the script the three roles are still members of %v, want none", got)
	}
	if got := scanInt(ctx, t, admin, `SELECT count(*) FROM pg_auth_members WHERE roleid = 'anbud_migrator'::regrole AND member = 'anbud_test_passed_on'::regrole`); got != 0 {
		t.Errorf("the membership the application passed on is still there (%d rows): the revoke did not cascade", got)
	}
}

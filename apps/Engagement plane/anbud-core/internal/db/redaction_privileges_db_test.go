package db_test

import (
	"context"
	"fmt"
	"slices"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// Who may do what with the objects of migration 0006, read back from a real server, in
// both orders in which the migrations and scripts/roles.sql can run. Needs a throwaway
// server: run with make test-db.

const redactionRegproc = "public.anbud_redact_raw_document(bigint,text,text)"

// redactionObjects are the two relations migration 0006 adds that the role model grants on.
var redactionObjects = []string{"raw_redactions", "raw_documents_live"}

// functionGrants lists the ACL entries of the redaction function as "grantee privilege",
// PUBLIC included, and nothing when the function has no ACL (every role may then run it).
func functionGrants(ctx context.Context, t *testing.T, pool *pgxpool.Pool) []string {
	t.Helper()
	return scanStrings(ctx, t, pool, `
		SELECT (CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee)::text END) || ' ' || a.privilege_type
		  FROM pg_proc p, aclexplode(p.proacl) a
		 WHERE p.oid = '`+redactionRegproc+`'::regprocedure
		 ORDER BY 1`)
}

// redactionPrivileges lists what the application, the DBA group and the owner hold on the
// new objects, one line per privilege held, in a fixed order.
func redactionPrivileges(ctx context.Context, t *testing.T, admin *pgxpool.Pool) []string {
	t.Helper()
	var held []string
	for _, role := range []string{dbtest.RoleApp, dbtest.RoleDBA} {
		for _, object := range redactionObjects {
			for _, priv := range tablePrivileges {
				if scanBool(ctx, t, admin, `SELECT has_table_privilege($1, $2, $3)`, role, "public."+object, priv) {
					held = append(held, fmt.Sprintf("%s %s %s", role, object, priv))
				}
			}
		}
		if role == dbtest.RoleDBA && scanBool(ctx, t, admin, `SELECT has_function_privilege($1, $2, 'EXECUTE')`, role, redactionRegproc) {
			held = append(held, role+" "+redactionFunctionName+" EXECUTE")
		}
	}
	if scanBool(ctx, t, admin, `SELECT has_function_privilege($1, $2, 'EXECUTE')`, dbtest.RoleMigrator, redactionRegproc) {
		held = append(held, dbtest.RoleMigrator+" "+redactionFunctionName+" EXECUTE")
	}
	return held
}

// wantRedactionPrivileges is what the application, the DBA group and the owner hold on the new
// objects whichever of the roles script and the migration ran first: the application reads the
// tombstones and the view (privileges.sql states it at every migration run), the group reads them
// and may run the function, and the owner may run it.
func wantRedactionPrivileges() []string {
	return []string{
		"anbud_app raw_redactions SELECT", "anbud_app raw_documents_live SELECT",
		"anbud_dba raw_redactions SELECT", "anbud_dba raw_documents_live SELECT",
		"anbud_dba " + redactionFunctionName + " EXECUTE",
		"anbud_migrator " + redactionFunctionName + " EXECUTE",
	}
}

// The function runs with its owner's rights, and nobody but the DBA group may start it:
// not the application, not a role in no group (which holds only what PUBLIC holds), and
// not PUBLIC itself, which has no entry in the function's ACL.
func TestRedactionFunctionIsExecutableByTheDBAGroupAndNoOneElse(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	nobody := dbtest.Pool(t, dbtest.CreateLoginRole(t, env.rs, nobodyLogin, ""))
	id := env.seedNotice(ctx, t, "2026-100001", contactBody(""))
	before := readDoc(ctx, t, env.admin, id)

	if got, want := functionGrants(ctx, t, env.admin), []string{"anbud_dba EXECUTE", "anbud_migrator EXECUTE"}; !slices.Equal(got, want) {
		t.Errorf("the function's ACL = %v, want %v: no PUBLIC entry, no anbud_app", got, want)
	}
	for _, c := range []struct {
		role string
		want bool
	}{{dbtest.RoleApp, false}, {dbtest.RoleDBA, true}, {dbtest.RoleMigrator, true}, {nobodyLogin, false}} {
		if got := scanBool(ctx, t, env.admin, `SELECT has_function_privilege($1, $2, 'EXECUTE')`, c.role, redactionRegproc); got != c.want {
			t.Errorf("%s EXECUTE on the redaction function = %v, want %v", c.role, got, c.want)
		}
	}

	for name, pool := range map[string]*pgxpool.Pool{"anbud_app": env.app, "a login role in no group": nobody} {
		var tomb int64
		err := pool.QueryRow(ctx, redactSQL, id, "erasure_request", "T-1").Scan(&tomb)
		if got := dbtest.Outcome(err); got != stateDenied {
			t.Errorf("a call as %s = %s, want %s", name, got, stateDenied)
		}
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_redactions`); got != 0 {
		t.Errorf("refused callers left %d tombstone(s)", got)
	}
	if after := readDoc(ctx, t, env.admin, id); after.RedactedAt != nil || len(after.Body) != len(before.Body) {
		t.Fatal("a refused caller changed the row")
	}

	// What the function is: the owner's, with a path that cannot be redirected.
	var secdef bool
	var owner string
	if err := env.admin.QueryRow(ctx, `SELECT prosecdef, pg_get_userbyid(proowner) FROM pg_proc WHERE oid = '`+redactionRegproc+`'::regprocedure`).Scan(&secdef, &owner); err != nil {
		t.Fatalf("read the function: %s", dbtest.Describe(err))
	}
	if !secdef || owner != dbtest.RoleMigrator {
		t.Errorf("the function is SECURITY DEFINER = %v and owned by %s, want true and %s", secdef, owner, dbtest.RoleMigrator)
	}
	if !scanBool(ctx, t, env.admin, `SELECT proconfig = ARRAY['search_path=pg_catalog, public, pg_temp'] FROM pg_proc WHERE oid = '`+redactionRegproc+`'::regprocedure`) {
		t.Error("the function's settings are not exactly search_path=pg_catalog, public, pg_temp")
	}

	// The group works: the call as a member succeeds, and as the owner as well.
	var tomb int64
	if err := env.dba.QueryRow(ctx, redactSQL, id, "erasure_request", "T-1").Scan(&tomb); err != nil {
		t.Errorf("a call as a member of anbud_dba: %s", dbtest.Describe(err))
	}
}

// The tombstones are read by the application and the DBA group and written by neither:
// only the function, running as the owner, inserts one. The view is read by both and
// written by neither. A role in no group reads neither.
func TestTombstonesAndTheViewAreReadOnlyForTheApplicationAndTheDBA(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	nobody := dbtest.Pool(t, dbtest.CreateLoginRole(t, env.rs, nobodyLogin, ""))
	redacted := env.seedNotice(ctx, t, "2026-100001", contactBody(""))
	live := env.seedNotice(ctx, t, "2026-100002", contactBody("<Live/>"))
	var tomb int64
	if err := env.dba.QueryRow(ctx, redactSQL, redacted, "erasure_request", "T-1").Scan(&tomb); err != nil {
		t.Fatalf("redact: %s", dbtest.Describe(err))
	}

	for name, pool := range map[string]*pgxpool.Pool{"anbud_app": env.app, "a member of anbud_dba": env.dba} {
		if got := scanInt(ctx, t, pool, `SELECT count(*) FROM raw_redactions`); got != 1 {
			t.Errorf("%s reads %d tombstones, want the 1 there is", name, got)
		}
		if got := scanInt(ctx, t, pool, `SELECT count(*) FROM raw_documents_live`); got != 1 {
			t.Errorf("%s reads %d rows from raw_documents_live, want only the one that still holds its body", name, got)
		}
		for what, sql := range map[string]string{
			"an INSERT of a tombstone":     `INSERT INTO raw_redactions (raw_document_id, doffin_id, original_sha256, original_size, reason, request_ref, redacted_by) VALUES (` + fmt.Sprint(live) + `, '2026-100002', repeat('a', 64), 10, 'objection', 'T-1', 'x')`,
			"an UPDATE of a tombstone":     `UPDATE raw_redactions SET reason = 'objection'`,
			"a DELETE of a tombstone":      `DELETE FROM raw_redactions`,
			"a TRUNCATE of the tombstones": `TRUNCATE raw_redactions`,
			"an INSERT through the view":   `INSERT INTO raw_documents_live (source, doffin_id, body, body_sha256, fetched_at, run_id) VALUES ('doffin', '2026-100009', 'x'::bytea, encode(sha256('x'::bytea), 'hex'), now(), ` + fmt.Sprint(env.run) + `)`,
			"an UPDATE through the view":   `UPDATE raw_documents_live SET content_type = 'x'`,
			"a DELETE through the view":    `DELETE FROM raw_documents_live`,
		} {
			if got := outcome(ctx, pool, sql); got != stateDenied {
				t.Errorf("%s as %s = %s, want %s", what, name, got, stateDenied)
			}
		}
	}
	for _, object := range redactionObjects {
		if got := outcome(ctx, nobody, `SELECT count(*) FROM `+object); got != stateDenied {
			t.Errorf("SELECT on %s as a role in no group = %s, want %s: PUBLIC holds nothing on it", object, got, stateDenied)
		}
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_redactions`); got != 1 {
		t.Errorf("tombstones = %d after the attempts, want the 1", got)
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_documents`); got != 2 {
		t.Errorf("raw_documents has %d rows after the attempts, want 2", got)
	}
}

// What the DBA group and the application hold on the new objects, the function's ACL
// and the refusal of the application, are the same whether the roles script or the
// migration ran first: when the migration ran first the roles script grants the application no
// table privilege by design, and privileges.sql, which the runner applies at every start, names
// the tombstones and the view, so the first run of the migrator under the model ends the same.
func TestRedactionPrivilegesAreTheSameWhicheverRunsFirstTheRolesScriptOrTheMigration(t *testing.T) {
	check := func(t *testing.T, env redactionEnv) {
		t.Helper()
		ctx := roleCtx(t)
		if got, want := redactionPrivileges(ctx, t, env.admin), wantRedactionPrivileges(); !slices.Equal(got, want) {
			t.Errorf("privileges on the redaction objects =\n  %s\nwant\n  %s", strings.Join(got, "\n  "), strings.Join(want, "\n  "))
		}
		if got, want := functionGrants(ctx, t, env.admin), []string{"anbud_dba EXECUTE", "anbud_migrator EXECUTE"}; !slices.Equal(got, want) {
			t.Errorf("the function's ACL = %v, want %v", got, want)
		}
		id := env.seedNotice(ctx, t, fmt.Sprintf("2026-%06d", 400000+env.run), contactBody(""))
		var tomb int64
		if err := env.app.QueryRow(ctx, redactSQL, id, "objection", "T-1").Scan(&tomb); dbtest.Outcome(err) != stateDenied {
			t.Errorf("the application role redacts = %s, want %s", dbtest.Outcome(err), stateDenied)
		}
		if err := env.dba.QueryRow(ctx, redactSQL, id, "objection", "T-1").Scan(&tomb); err != nil {
			t.Errorf("a member of anbud_dba cannot redact: %s", dbtest.Describe(err))
		}
		if got := outcome(ctx, env.app, `INSERT INTO raw_redactions (raw_document_id, doffin_id, original_sha256, original_size, reason, request_ref, redacted_by) VALUES (1, '2026-100002', repeat('a', 64), 10, 'objection', 'T-1', 'x')`); got != stateDenied {
			t.Errorf("the application role writes a tombstone = %s, want %s", got, stateDenied)
		}
	}

	t.Run("the roles script first, then the migration", func(t *testing.T) {
		rs, admin := migratedRoles(t)
		env := redactionEnvOn(t, rs, admin)
		check(t, env)

		// Another run of the roles script and of the runner leaves it as it is.
		dbtest.RunRolesScript(t, rs)
		if err := db.ApplyMigrations(roleCtx(t), dbtest.Pool(t, rs.MigratorDSN)); err != nil {
			t.Fatalf("ApplyMigrations as the migrator: %s", dbtest.Describe(err))
		}
		if got, want := redactionPrivileges(roleCtx(t), t, admin), wantRedactionPrivileges(); !slices.Equal(got, want) {
			t.Errorf("after the script and the runner ran again, privileges =\n  %s\nwant\n  %s", strings.Join(got, "\n  "), strings.Join(want, "\n  "))
		}
	})

	t.Run("the migration first, then the roles script", func(t *testing.T) {
		dsn := dbtest.Database(t) // migrated to 0006 by the server's role, with no anbud_ role in the cluster
		admin := dbtest.Pool(t, dsn)
		ctx := roleCtx(t)
		if got := functionGrants(ctx, t, admin); len(got) != 1 {
			t.Fatalf("before the script the function's ACL = %v, want the owner alone", got)
		}
		rs := dbtest.RolesOn(t, dsn)
		if err := db.ApplyMigrations(ctx, dbtest.Pool(t, rs.MigratorDSN)); err != nil {
			t.Fatalf("ApplyMigrations as the migrator on the taken-over database: %s", dbtest.Describe(err))
		}
		check(t, redactionEnvOn(t, rs, admin))

		before := aclSnapshot(ctx, t, admin)
		dbtest.RunRolesScript(t, rs)
		if after := aclSnapshot(ctx, t, admin); !slices.Equal(before, after) {
			t.Errorf("a second run of the roles script changed the ACLs:\nbefore:\n%s\nafter:\n%s", strings.Join(before, "\n"), strings.Join(after, "\n"))
		}
	})
}

// Running the roles script again undoes a change someone made to who may run the
// function: a grant to the application or to PUBLIC, and a revoke from the DBA group.
func TestRolesScriptCorrectsDriftOnTheRedactionFunction(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	before := aclSnapshot(ctx, t, admin)

	for _, sql := range []string{
		`GRANT EXECUTE ON FUNCTION anbud_redact_raw_document(bigint, text, text) TO anbud_app, PUBLIC`,
		`REVOKE EXECUTE ON FUNCTION anbud_redact_raw_document(bigint, text, text) FROM anbud_dba`,
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
	if got, want := functionGrants(ctx, t, admin), []string{"anbud_dba EXECUTE", "anbud_migrator EXECUTE"}; !slices.Equal(got, want) {
		t.Errorf("the function's ACL after the script = %v, want %v", got, want)
	}
}

// The migration works on a database where no role of the role model exists: it makes the
// function, leaves it to its owner alone, and the owner can run it.
func TestMigration0006WorksOnADatabaseWithNoRoleModel(t *testing.T) {
	pool := dbtest.Open(t)
	ctx := roleCtx(t)
	if n := scanInt(ctx, t, pool, `SELECT count(*) FROM pg_roles WHERE rolname IN ('anbud_migrator', 'anbud_app', 'anbud_dba')`); n != 0 {
		t.Fatalf("%d roles of the role model exist in the cluster; this test needs a cluster without them", n)
	}

	if got := functionGrants(ctx, t, pool); len(got) != 1 || !strings.HasSuffix(got[0], " EXECUTE") || strings.HasPrefix(got[0], "PUBLIC") {
		t.Errorf("the function's ACL = %v, want its owner alone", got)
	}
	var run, doc, tomb int64
	if err := pool.QueryRow(ctx, beginRunSQL).Scan(&run); err != nil {
		t.Fatalf("begin a run: %s", dbtest.Describe(err))
	}
	if err := pool.QueryRow(ctx, insertNoticeSQL, "2026-100001", contactBody(""), run).Scan(&doc); err != nil {
		t.Fatalf("store a notice: %s", dbtest.Describe(err))
	}
	if err := pool.QueryRow(ctx, redactSQL, doc, "erasure_request", "T-1").Scan(&tomb); err != nil {
		t.Fatalf("redact as the owner on a database with no role model: %s", dbtest.Describe(err))
	}
	if after := readDoc(ctx, t, pool, doc); len(after.Body) != 0 || after.RedactedAt == nil {
		t.Error("the row is not redacted")
	}
}

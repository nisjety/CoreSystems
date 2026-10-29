package db_test

import (
	"errors"
	"slices"
	"strconv"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// What PUBLIC holds on the database, and what the script refuses to prepare. PUBLIC holds
// CONNECT and TEMPORARY on every database unless it is told not to. Any role of the cluster
// could then connect and take the budget lock of the sync, so that every sync and probe
// exited 7, and could create a temporary table named like a table of the schema, which
// shadows the name wherever a function does not qualify it: the run guard of migration 0005
// reads sync_runs unqualified. Needs a throwaway server: run with make test-db.

// A role that nobody granted anything cannot connect. The three roles of the model connect
// because the script names them, and so does a role a test gives CONNECT by name.
func TestARoleWithNoGrantsCannotConnectAndPublicHoldsNothingOnTheDatabase(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	stranger := dbtest.CreateLoginRoleWithoutConnect(t, rs, "anbud_test_stranger")

	for _, priv := range []string{"CONNECT", "TEMPORARY", "CREATE"} {
		if scanBool(ctx, t, admin, `SELECT has_database_privilege('anbud_test_stranger', current_database(), $1)`, priv) {
			t.Errorf("a role with no grants holds %s on the database: PUBLIC still gives it", priv)
		}
	}
	if scanBool(ctx, t, admin, `SELECT EXISTS (SELECT 1 FROM pg_database d, aclexplode(d.datacl) a WHERE d.datname = current_database() AND a.grantee = 0)`) {
		t.Error("PUBLIC holds a privilege on the database")
	}

	dbtest.MustBeThrowaway(t, stranger)
	conn, err := pgx.Connect(ctx, stranger)
	if err == nil {
		_ = conn.Close(ctx)
		t.Fatal("a role with no grants connected to the database")
	}
	if got := dbtest.SQLState(err); got != stateDenied {
		t.Errorf("a role with no grants connecting = SQLSTATE %s, want %s from the server: no privilege to connect", got, stateDenied)
	}

	// The roles that must connect still do, by their explicit grants.
	dba := dbtest.CreateLoginRole(t, rs, "anbud_test_connects", "", dbtest.RoleDBA)
	for name, dsn := range map[string]string{"anbud_app": rs.AppDSN, "anbud_migrator": rs.MigratorDSN, "a member of anbud_dba": dba} {
		if err := dbtest.Pool(t, dsn).Ping(ctx); err != nil {
			t.Errorf("%s cannot connect: %s", name, dbtest.Describe(err))
		}
	}
}

// The application has no TEMPORARY privilege and so cannot create a temporary table; a member of
// the DBA group has it through the group, and the owner has every privilege of its database. A
// role in no group has none either.
func TestApplicationCannotCreateATemporaryTableButTheDBAGroupCan(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	nobody := dbtest.Pool(t, dbtest.CreateLoginRole(t, env.rs, nobodyLogin, ""))

	for _, c := range []struct {
		role string
		want bool
	}{{dbtest.RoleApp, false}, {dbtest.RoleDBA, true}, {dbtest.RoleMigrator, true}, {nobodyLogin, false}} {
		if got := scanBool(ctx, t, env.admin, `SELECT has_database_privilege($1, current_database(), 'TEMPORARY')`, c.role); got != c.want {
			t.Errorf("%s TEMPORARY on the database = %v, want %v", c.role, got, c.want)
		}
	}
	const temp = `CREATE TEMP TABLE anbud_probe (id int)`
	if got := outcome(ctx, env.app, temp); got != stateDenied {
		t.Errorf("a temporary table as anbud_app = %s, want %s", got, stateDenied)
	}
	if got := outcome(ctx, nobody, temp); got != stateDenied {
		t.Errorf("a temporary table as a role in no group = %s, want %s", got, stateDenied)
	}
	if got := outcome(ctx, env.dba, temp); got != stateOK {
		t.Errorf("a temporary table as a member of anbud_dba = %s, want ok", got)
	}
	if got := outcome(ctx, env.owner, temp); got != stateOK {
		t.Errorf("a temporary table as the owner = %s, want ok", got)
	}
}

// Running the script again takes back what someone granted by hand on the database: TEMPORARY
// and CREATE to the application, CREATE to the DBA group, CONNECT, TEMPORARY and CREATE to
// PUBLIC. The DBA group keeps TEMPORARY. Afterwards every ACL is what it was.
func TestRolesScriptTakesBackDatabasePrivilegesGrantedByHand(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	before := aclSnapshot(ctx, t, admin)
	db := ident(rs.Database)

	for _, sql := range []string{
		`GRANT TEMPORARY, CREATE ON DATABASE ` + db + ` TO anbud_app`,
		`GRANT CREATE ON DATABASE ` + db + ` TO anbud_dba`,
		`GRANT CONNECT, TEMPORARY, CREATE ON DATABASE ` + db + ` TO PUBLIC`,
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
	for _, c := range []struct {
		role, priv string
		want       bool
	}{
		{dbtest.RoleApp, "TEMPORARY", false}, {dbtest.RoleApp, "CREATE", false}, {dbtest.RoleApp, "CONNECT", true},
		{dbtest.RoleDBA, "TEMPORARY", true}, {dbtest.RoleDBA, "CREATE", false}, {dbtest.RoleDBA, "CONNECT", true},
	} {
		if got := scanBool(ctx, t, admin, `SELECT has_database_privilege($1, current_database(), $2)`, c.role, c.priv); got != c.want {
			t.Errorf("%s %s on the database after the script = %v, want %v", c.role, c.priv, got, c.want)
		}
	}
}

// The attack the TEMPORARY privilege made possible. The run guard of migration 0005 reads
// sync_runs unqualified, a temporary table named sync_runs is searched before the schema, and a
// role that may create one can claim any finished run is running, so that evidence is accepted
// under it. The control shows the attack works while the application holds TEMPORARY, as it did
// when PUBLIC gave it. After the script has taken the privilege away the table cannot be made,
// and the guard refuses the finished run again.
func TestATemporaryTableCannotMakeTheRunGuardAcceptAFinishedRun(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	app := dbtest.Pool(t, rs.AppDSN)
	runID := seedAsApp(ctx, t, app)
	if got := outcome(ctx, app, finishRunSQL, runID); got != stateOK {
		t.Fatalf("finish the run = %s, want ok", got)
	}
	if got := outcome(ctx, app, seedPageSQL, runID, []byte(`{"hits":[1]}`)); got != stateNotReady {
		t.Fatalf("evidence under a finished run = %s, want %s from the run guard", got, stateNotReady)
	}

	// The control: the privilege PUBLIC used to hand every role.
	if _, err := admin.Exec(ctx, `GRANT TEMPORARY ON DATABASE `+ident(rs.Database)+` TO anbud_app`); err != nil {
		t.Fatalf("arrange: %s", dbtest.Describe(err))
	}
	conn, err := app.Acquire(ctx)
	if err != nil {
		t.Fatalf("acquire: %s", dbtest.Describe(err))
	}
	for _, sql := range []string{
		`CREATE TEMP TABLE sync_runs (id bigint, state text)`,
		`INSERT INTO sync_runs VALUES (` + strconv.FormatInt(runID, 10) + `, 'running')`,
	} {
		if _, err := conn.Exec(ctx, sql); err != nil {
			t.Fatalf("control: %q as the application with TEMPORARY: %s", sql, dbtest.Describe(err))
		}
	}
	if _, err := conn.Exec(ctx, seedPageSQL, runID, []byte(`{"hits":[2]}`)); err != nil {
		t.Fatalf("control: the shadowing table did not make the run guard accept a finished run (%s), so the test below would prove nothing", dbtest.Describe(err))
	}
	if _, err := conn.Exec(ctx, `DISCARD TEMP`); err != nil {
		t.Fatalf("discard the temporary table: %s", dbtest.Describe(err))
	}
	conn.Release()

	dbtest.RunRolesScript(t, rs)

	conn, err = app.Acquire(ctx)
	if err != nil {
		t.Fatalf("acquire: %s", dbtest.Describe(err))
	}
	defer conn.Release()
	_, err = conn.Exec(ctx, `CREATE TEMP TABLE sync_runs (id bigint, state text)`)
	if got := dbtest.Outcome(err); got != stateDenied {
		t.Errorf("a temporary table named sync_runs after the script = %s, want %s", got, stateDenied)
	}
	_, err = conn.Exec(ctx, seedPageSQL, runID, []byte(`{"hits":[3]}`))
	if got := dbtest.Outcome(err); got != stateNotReady {
		t.Errorf("evidence under a finished run after the script = %s, want %s from the run guard", got, stateNotReady)
	}
}

// The script refuses a database whose name does not start with anbud, before it changes anything:
// the roles have fixed names and belong to the whole cluster, so a command line that names some
// other database must not create them there. The server's own database, postgres on the throwaway
// container, is such a database.
func TestRolesScriptRefusesADatabaseWhoseNameDoesNotStartWithAnbud(t *testing.T) {
	server := dbtest.ServerDSN(t)
	ctx := roleCtx(t)
	admin := dbtest.Pool(t, server)
	ownerBefore := scanStrings(ctx, t, admin, `SELECT pg_get_userbyid(datdba) FROM pg_database WHERE datname = current_database()`)
	aclBefore := scanStrings(ctx, t, admin, `SELECT coalesce(datacl::text, '-') FROM pg_database WHERE datname = current_database()`)

	err := dbtest.TryRolesScriptOnServerDatabase(t)

	var pgErr *pgconn.PgError
	if err == nil {
		t.Fatal("the script ran on a database whose name does not start with anbud")
	}
	if !errors.As(err, &pgErr) || pgErr.Code != "P0001" || !strings.Contains(pgErr.Message, "must start with anbud") {
		t.Fatalf("the script failed with %s, want its own refusal (P0001, the name must start with anbud)", dbtest.Describe(err))
	}
	if n := scanInt(ctx, t, admin, `SELECT count(*) FROM pg_roles WHERE rolname IN `+rolesByName); n != 0 {
		t.Errorf("the refused script left %d of the roles in the cluster, want none", n)
	}
	if got := scanStrings(ctx, t, admin, `SELECT pg_get_userbyid(datdba) FROM pg_database WHERE datname = current_database()`); !slices.Equal(got, ownerBefore) {
		t.Errorf("the refused script changed the database's owner: %v, was %v", got, ownerBefore)
	}
	if got := scanStrings(ctx, t, admin, `SELECT coalesce(datacl::text, '-') FROM pg_database WHERE datname = current_database()`); !slices.Equal(got, aclBefore) {
		t.Errorf("the refused script changed the database's privileges: %v, were %v", got, aclBefore)
	}
}

// The first guard still holds: -v dbname must be the database of the connection. Run against
// another anbud database it changes nothing.
func TestRolesScriptRefusesADatabaseNameThatIsNotTheConnection(t *testing.T) {
	_, admin := migratedRoles(t)
	ctx := roleCtx(t)
	before := aclSnapshot(ctx, t, admin)

	script := dbtest.ExpandRolesScript(dbtest.ReadRolesScript(t), "anbud_test_another_database")
	_, err := admin.Exec(ctx, script)

	var pgErr *pgconn.PgError
	if !errors.As(err, &pgErr) || pgErr.Code != "P0001" || !strings.Contains(pgErr.Message, "not the database") {
		t.Fatalf("the script failed with %s, want its own refusal (P0001, not the database that -v dbname names)", dbtest.Describe(err))
	}
	if after := aclSnapshot(ctx, t, admin); !slices.Equal(before, after) {
		t.Errorf("the refused script changed the ACLs:\nbefore:\n%s\nafter:\n%s", strings.Join(before, "\n"), strings.Join(after, "\n"))
	}
}

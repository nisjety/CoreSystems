package db_test

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"slices"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// Every reason the runtime-role check has, read from a real server: a role is judged by what it
// can use or become, so membership of a role counts as much as the role's own attributes,
// and ownership counts for every kind of object and for the schema and the database, not for
// tables alone. Each case arranges one thing, connects as the role, and says which facts are
// true, exactly, and what the check does with them: a warning on loopback and a refusal
// on any other host. Needs a throwaway server: run with make test-db.

// trueFacts names the facts that are true, in the order of the fields.
func trueFacts(f db.RoleFacts) []string {
	var out []string
	v := reflect.ValueOf(f)
	for i := range v.NumField() {
		if v.Field(i).Bool() {
			out = append(out, v.Type().Field(i).Name)
		}
	}
	return out
}

// sameFacts reports whether the facts that are true are exactly want, whatever the order.
func sameFacts(got db.RoleFacts, want []string) bool {
	names := trueFacts(got)
	slices.Sort(names)
	w := slices.Clone(want)
	slices.Sort(w)
	return slices.Equal(names, w)
}

// hasFacts reports whether every fact in want is true; others may be true as well.
func hasFacts(got db.RoleFacts, want []string) bool {
	names := trueFacts(got)
	for _, w := range want {
		if !slices.Contains(names, w) {
			return false
		}
	}
	return true
}

// reasonEnv is where a case arranges: a database prepared by scripts/roles.sql and migrated, and a
// pool for the server's own role, which arranges.
type reasonEnv struct {
	rs    dbtest.RoleSet
	admin *pgxpool.Pool
}

func (e reasonEnv) exec(ctx context.Context, t *testing.T, stmts ...string) {
	t.Helper()
	for _, stmt := range stmts {
		if _, err := e.admin.Exec(ctx, stmt); err != nil {
			t.Fatalf("arrange %q: %s", stmt, dbtest.Describe(err))
		}
	}
}

// reasonCase is one way for a role to be too privileged, or not to be.
type reasonCase struct {
	name string
	// arrange makes the role and what it holds, and returns the DSN to connect with.
	arrange func(ctx context.Context, t *testing.T, e reasonEnv) string
	want    []string // facts that must be true
	exact   bool     // when set, no other fact may be true
}

func (c reasonCase) run(ctx context.Context, t *testing.T, e reasonEnv) {
	t.Helper()
	dsn := c.arrange(ctx, t, e)
	pool := dbtest.Pool(t, dsn)

	got, err := db.ReadRoleFacts(ctx, pool)
	if err != nil {
		t.Fatalf("ReadRoleFacts: %s", dbtest.Describe(err))
	}
	switch {
	case c.exact && !sameFacts(got, c.want):
		t.Fatalf("facts that are true = %v, want exactly %v", trueFacts(got), c.want)
	case !c.exact && !hasFacts(got, c.want):
		t.Fatalf("facts that are true = %v, want at least %v", trueFacts(got), c.want)
	}

	verdict, err := db.CheckRuntimeRole(ctx, pool, dsn) // the server is on loopback
	switch {
	case got.Excessive() && (err != nil || verdict != db.RoleWarn):
		t.Errorf("loopback: got (%v, %v), want (RoleWarn, nil)", verdict, err)
	case !got.Excessive() && (err != nil || verdict != db.RoleOK):
		t.Errorf("loopback: got (%v, %v), want (RoleOK, nil)", verdict, err)
	}
	verdict, err = db.CheckRuntimeRole(ctx, pool, remoteDSN)
	switch {
	case got.Excessive() && !errors.Is(err, db.ErrRoleTooPrivileged):
		t.Errorf("remote: error = %v, want ErrRoleTooPrivileged", err)
	case !got.Excessive() && (err != nil || verdict != db.RoleOK):
		t.Errorf("remote: got (%v, %v), want (RoleOK, nil)", verdict, err)
	}
}

func ident(name string) string { return pgx.Identifier{name}.Sanitize() }

// plainDatabase is a migrated database that scripts/roles.sql has not touched, so a case
// may change who owns its schema or the database itself without disturbing any other case.
// Its roles go with it.
func plainDatabase(t *testing.T) reasonEnv {
	t.Helper()
	dsn := dbtest.Database(t)
	cfg, err := pgx.ParseConfig(dsn)
	if err != nil {
		t.Fatal("unparsable DSN")
	}
	return reasonEnv{rs: dbtest.RoleSet{Database: cfg.Database, AdminDSN: dsn}, admin: dbtest.Pool(t, dsn)}
}

func serverVersion(ctx context.Context, t *testing.T, admin *pgxpool.Pool) int {
	t.Helper()
	var n int
	if err := admin.QueryRow(ctx, `SELECT current_setting('server_version_num')::int`).Scan(&n); err != nil {
		t.Fatalf("read the server version: %s", dbtest.Describe(err))
	}
	return n
}

// Everything that makes a role too privileged, one case each, and the roles that are not.
func TestRuntimeRoleCheckReasonsAgainstRealRoles(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	e := reasonEnv{rs: rs, admin: admin}
	adminRole := scanStrings(ctx, t, admin, `SELECT current_user::text`)[0]
	schemaViaOwner := schemaIsOwnedByTheDatabaseOwner(ctx, t, admin)

	login := func(name string, attr string, memberOf ...string) func(context.Context, *testing.T, reasonEnv) string {
		return func(_ context.Context, t *testing.T, e reasonEnv) string {
			return dbtest.CreateLoginRole(t, e.rs, name, attr, memberOf...)
		}
	}
	// ownedBy makes a login role, creates an object as the server's role and gives it to the role.
	ownedBy := func(name string, create string, alter string) func(context.Context, *testing.T, reasonEnv) string {
		return func(ctx context.Context, t *testing.T, e reasonEnv) string {
			dsn := dbtest.CreateLoginRole(t, e.rs, name, "")
			e.exec(ctx, t, create, fmt.Sprintf(alter, ident(name)))
			return dsn
		}
	}

	migratorMember := []string{"HoldsMigratorRole", "OwnsRelation", "OwnsFunction", "OwnsDatabase"}
	if schemaViaOwner {
		migratorMember = append(migratorMember, "OwnsSchema")
	}

	cases := []reasonCase{
		// Roles with nothing to answer for.
		{name: "the application role", arrange: func(_ context.Context, _ *testing.T, e reasonEnv) string { return e.rs.AppDSN }, exact: true},
		{name: "a role that holds nothing at all", arrange: login("anbud_test_r_stranger", ""), exact: true},

		// Attributes held through a role the role belongs to.
		{name: "a member of a role with CREATEROLE", exact: true, want: []string{"CreateRole"},
			arrange: func(ctx context.Context, t *testing.T, e reasonEnv) string {
				dbtest.CreateLoginRole(t, e.rs, "anbud_test_g_createrole", "CREATEROLE")
				return dbtest.CreateLoginRole(t, e.rs, "anbud_test_r_createrole", "", "anbud_test_g_createrole")
			}},
		{name: "a member of a role with CREATEDB", exact: true, want: []string{"CreateDB"},
			arrange: func(ctx context.Context, t *testing.T, e reasonEnv) string {
				dbtest.CreateLoginRole(t, e.rs, "anbud_test_g_createdb", "CREATEDB")
				return dbtest.CreateLoginRole(t, e.rs, "anbud_test_r_createdb", "", "anbud_test_g_createdb")
			}},
		{name: "a member of a role with BYPASSRLS", exact: true, want: []string{"BypassRLS"},
			arrange: func(ctx context.Context, t *testing.T, e reasonEnv) string {
				dbtest.CreateLoginRole(t, e.rs, "anbud_test_g_bypassrls", "BYPASSRLS")
				return dbtest.CreateLoginRole(t, e.rs, "anbud_test_r_bypassrls", "", "anbud_test_g_bypassrls")
			}},
		// The server's own role is a superuser, with every attribute and the catalogs, so more than
		// one fact is true for a member of it; the superuser one is the point.
		{name: "a member of a superuser", want: []string{"Superuser"},
			arrange: login("anbud_test_r_super", "", adminRole)},

		// Membership that does not inherit: the role cannot use what the group holds without a
		// SET ROLE, and it can SET ROLE, so it can become the group. A check by inheritance would miss it.
		{name: "a member that does not inherit but can SET ROLE", exact: true, want: []string{"CreateRole"},
			arrange: func(ctx context.Context, t *testing.T, e reasonEnv) string {
				if serverVersion(ctx, t, e.admin) < 160000 {
					t.Skip("GRANT ... WITH INHERIT FALSE needs PostgreSQL 16")
				}
				dbtest.CreateLoginRole(t, e.rs, "anbud_test_g_noinherit", "CREATEROLE")
				dsn := dbtest.CreateLoginRole(t, e.rs, "anbud_test_r_noinherit", "", "anbud_test_g_noinherit")
				e.exec(ctx, t,
					`REVOKE anbud_test_g_noinherit FROM anbud_test_r_noinherit`,
					`GRANT anbud_test_g_noinherit TO anbud_test_r_noinherit WITH INHERIT FALSE, SET TRUE`)
				if scanBool(ctx, t, e.admin, `SELECT pg_has_role('anbud_test_r_noinherit', 'anbud_test_g_noinherit', 'USAGE')`) {
					t.Fatal("the role inherits the group; the case would not tell membership from inheritance")
				}
				if !scanBool(ctx, t, e.admin, `SELECT pg_has_role('anbud_test_r_noinherit', 'anbud_test_g_noinherit', 'MEMBER')`) {
					t.Fatal("the role is not a member of the group")
				}
				return dsn
			}},

		// The roles of the model: the application must be a member of neither.
		{name: "a member of the owner role", exact: true, want: migratorMember,
			arrange: login("anbud_test_r_migrator_member", "", dbtest.RoleMigrator)},
		{name: "a member of the DBA group", exact: true, want: []string{"HoldsDBARole"},
			arrange: login("anbud_test_r_dba_member", "", dbtest.RoleDBA)},

		// Ownership of every kind of relation, and of functions and procedures.
		{name: "the owner of a table", exact: true, want: []string{"OwnsRelation"},
			arrange: ownedBy("anbud_test_r_table", `CREATE TABLE public.anbud_test_owned_table (id int)`, `ALTER TABLE public.anbud_test_owned_table OWNER TO %s`)},
		{name: "the owner of a partitioned table", exact: true, want: []string{"OwnsRelation"},
			arrange: ownedBy("anbud_test_r_part", `CREATE TABLE public.anbud_test_owned_part (id int) PARTITION BY RANGE (id)`, `ALTER TABLE public.anbud_test_owned_part OWNER TO %s`)},
		{name: "the owner of a view", exact: true, want: []string{"OwnsRelation"},
			arrange: ownedBy("anbud_test_r_view", `CREATE VIEW public.anbud_test_owned_view AS SELECT 1 AS one`, `ALTER VIEW public.anbud_test_owned_view OWNER TO %s`)},
		{name: "the owner of a materialized view", exact: true, want: []string{"OwnsRelation"},
			arrange: ownedBy("anbud_test_r_matview", `CREATE MATERIALIZED VIEW public.anbud_test_owned_matview AS SELECT 1 AS one`, `ALTER MATERIALIZED VIEW public.anbud_test_owned_matview OWNER TO %s`)},
		{name: "the owner of a sequence", exact: true, want: []string{"OwnsRelation"},
			arrange: ownedBy("anbud_test_r_sequence", `CREATE SEQUENCE public.anbud_test_owned_sequence`, `ALTER SEQUENCE public.anbud_test_owned_sequence OWNER TO %s`)},
		{name: "the owner of a foreign table", exact: true, want: []string{"OwnsRelation"},
			arrange: func(ctx context.Context, t *testing.T, e reasonEnv) string {
				dsn := dbtest.CreateLoginRole(t, e.rs, "anbud_test_r_foreign", "")
				e.exec(ctx, t,
					`CREATE FOREIGN DATA WRAPPER anbud_test_fdw`,
					`CREATE SERVER anbud_test_server FOREIGN DATA WRAPPER anbud_test_fdw`,
					`CREATE FOREIGN TABLE public.anbud_test_owned_foreign (id int) SERVER anbud_test_server`,
					`ALTER FOREIGN TABLE public.anbud_test_owned_foreign OWNER TO anbud_test_r_foreign`)
				return dsn
			}},
		{name: "the owner of a function", exact: true, want: []string{"OwnsFunction"},
			arrange: ownedBy("anbud_test_r_function", `CREATE FUNCTION public.anbud_test_owned_fn() RETURNS int LANGUAGE sql AS 'SELECT 1'`, `ALTER FUNCTION public.anbud_test_owned_fn() OWNER TO %s`)},
		{name: "the owner of a procedure", exact: true, want: []string{"OwnsFunction"},
			arrange: ownedBy("anbud_test_r_procedure", `CREATE PROCEDURE public.anbud_test_owned_proc() LANGUAGE sql AS 'SELECT 1'`, `ALTER PROCEDURE public.anbud_test_owned_proc() OWNER TO %s`)},
		{name: "the owner of a view through a group it belongs to", exact: true, want: []string{"OwnsRelation"},
			arrange: func(ctx context.Context, t *testing.T, e reasonEnv) string {
				dbtest.CreateLoginRole(t, e.rs, "anbud_test_g_viewowner", "")
				dsn := dbtest.CreateLoginRole(t, e.rs, "anbud_test_r_viewmember", "", "anbud_test_g_viewowner")
				e.exec(ctx, t, `CREATE VIEW public.anbud_test_group_view AS SELECT 1 AS one`, `ALTER VIEW public.anbud_test_group_view OWNER TO anbud_test_g_viewowner`)
				return dsn
			}},

		// A schema off the role's search path is not one its names resolve in.
		{name: "the owner of a schema that is not on its search path", exact: true,
			arrange: func(ctx context.Context, t *testing.T, e reasonEnv) string {
				dsn := dbtest.CreateLoginRole(t, e.rs, "anbud_test_r_elsewhere", "")
				e.exec(ctx, t, `CREATE SCHEMA anbud_test_elsewhere AUTHORIZATION anbud_test_r_elsewhere`)
				return dsn
			}},

		// A temporary table is session-local and vanishes with its session; the role owns it, and
		// the catalog shows it to every other session, but it is not an object the role keeps.
		{name: "a role that owns a temporary table in another session", exact: true,
			arrange: func(ctx context.Context, t *testing.T, e reasonEnv) string {
				dsn := dbtest.CreateLoginRole(t, e.rs, "anbud_test_r_temp", "")
				e.exec(ctx, t, `GRANT TEMPORARY ON DATABASE `+ident(e.rs.Database)+` TO anbud_test_r_temp`)
				holder := dbtest.Pool(t, dsn)
				conn, err := holder.Acquire(ctx)
				if err != nil {
					t.Fatalf("connect as the role: %s", dbtest.Describe(err))
				}
				t.Cleanup(conn.Release)
				if _, err := conn.Exec(ctx, `CREATE TEMP TABLE anbud_test_scratch (id int)`); err != nil {
					t.Fatalf("create a temporary table as the role: %s", dbtest.Describe(err))
				}
				if !scanBool(ctx, t, e.admin, `SELECT EXISTS (SELECT 1 FROM pg_class c WHERE c.relpersistence = 't' AND pg_get_userbyid(c.relowner) = 'anbud_test_r_temp')`) {
					t.Fatal("the temporary table is not in the catalog; the case would prove nothing")
				}
				return dsn
			}},
	}

	// Each predefined role that crosses the boundary and that this server has.
	for _, predefined := range []string{"pg_execute_server_program", "pg_write_server_files", "pg_read_server_files", "pg_write_all_data", "pg_signal_backend", "pg_maintain", "pg_checkpoint"} {
		if !scanBool(ctx, t, admin, `SELECT to_regrole($1) IS NOT NULL`, predefined) {
			t.Logf("this server has no %s; its case is left out", predefined)
			continue
		}
		cases = append(cases, reasonCase{
			name: "a member of " + predefined, exact: true, want: []string{"HoldsPredefinedRole"},
			arrange: login("anbud_test_r_"+predefined, "", predefined),
		})
	}

	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) { c.run(ctx, t, e) })
	}
}

// Ownership of the schema and of the database each need a database of their own to arrange:
// the owner of the schema public owns the schema, and the owner of the database owns it
// and, from PostgreSQL 15, the schema as well, through pg_database_owner.
func TestRuntimeRoleCheckSeesOwnershipOfTheSchemaAndTheDatabase(t *testing.T) {
	ctx := roleCtx(t)

	t.Run("the owner of the schema public", func(t *testing.T) {
		e := plainDatabase(t)
		reasonCase{
			name: "schema", exact: true, want: []string{"OwnsSchema"},
			arrange: func(ctx context.Context, t *testing.T, e reasonEnv) string {
				dsn := dbtest.CreateLoginRole(t, e.rs, "anbud_test_r_schema", "")
				e.exec(ctx, t, `ALTER SCHEMA public OWNER TO anbud_test_r_schema`)
				return dsn
			},
		}.run(ctx, t, e)
	})

	t.Run("the owner of the database, which owns the schema through pg_database_owner", func(t *testing.T) {
		e := plainDatabase(t)
		want := []string{"OwnsDatabase"}
		if schemaIsOwnedByTheDatabaseOwner(ctx, t, e.admin) {
			want = append(want, "OwnsSchema")
		}
		reasonCase{
			name: "database", exact: true, want: want,
			arrange: func(ctx context.Context, t *testing.T, e reasonEnv) string {
				dsn := dbtest.CreateLoginRole(t, e.rs, "anbud_test_r_database", "")
				e.exec(ctx, t, `ALTER DATABASE `+ident(e.rs.Database)+` OWNER TO anbud_test_r_database`)
				return dsn
			},
		}.run(ctx, t, e)
	})
}

// A membership added to the application role by hand, which the old check could not see,
// is what the finding was about: the role is then too privileged although it has no attribute
// and owns nothing. Each of these is a statement a DBA could run, in a hurry or by mistake.
func TestRuntimeRoleCheckSeesAMembershipGrantedToTheApplicationRoleByHand(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)
	app := dbtest.Pool(t, rs.AppDSN)

	if got, err := db.ReadRoleFacts(ctx, app); err != nil || got.Excessive() {
		t.Fatalf("before any grant the application role has facts %v (error %v), want none", trueFacts(got), err)
	}
	for _, c := range []struct {
		grant string
		want  []string
	}{
		{"pg_execute_server_program", []string{"HoldsPredefinedRole"}},
		{dbtest.RoleMigrator, []string{"HoldsMigratorRole", "OwnsRelation", "OwnsFunction", "OwnsDatabase"}},
		{dbtest.RoleDBA, []string{"HoldsDBARole"}},
	} {
		t.Run("GRANT "+c.grant+" TO anbud_app", func(t *testing.T) {
			if _, err := admin.Exec(ctx, `GRANT `+ident(c.grant)+` TO anbud_app`); err != nil {
				t.Fatalf("grant: %s", dbtest.Describe(err))
			}
			t.Cleanup(func() {
				_, _ = admin.Exec(context.Background(), `REVOKE `+ident(c.grant)+` FROM anbud_app`)
			})

			got, err := db.ReadRoleFacts(ctx, app)
			if err != nil {
				t.Fatalf("ReadRoleFacts: %s", dbtest.Describe(err))
			}
			if !hasFacts(got, c.want) {
				t.Errorf("facts that are true = %v, want at least %v", trueFacts(got), c.want)
			}
			if _, err := db.CheckRuntimeRole(ctx, app, remoteDSN); !errors.Is(err, db.ErrRoleTooPrivileged) {
				t.Errorf("remote: error = %v, want ErrRoleTooPrivileged", err)
			}
			if verdict, err := db.CheckRuntimeRole(ctx, app, rs.AppDSN); err != nil || verdict != db.RoleWarn {
				t.Errorf("loopback: got (%v, %v), want (RoleWarn, nil)", verdict, err)
			}
		})
	}
	if got, err := db.ReadRoleFacts(ctx, app); err != nil || got.Excessive() {
		t.Errorf("after the grants were taken back the application role has facts %v (error %v), want none", trueFacts(got), err)
	}
}

// The owner role is checked on the attributes that let a role escape, and not on ownership,
// which it has by design. A role that owns and holds the model's owner role passes; one that is a
// superuser, or belongs to a CREATEROLE role or to a predefined role that reaches programs, does not.
func TestOwnerRoleCheckAgainstRealRoles(t *testing.T) {
	rs, _ := migratedRoles(t)
	ctx := roleCtx(t)

	for _, c := range []struct {
		name    string
		dsn     string
		refused bool
	}{
		{"the owner role", rs.MigratorDSN, false},
		{"a superuser", rs.AdminDSN, true},
		{"a role with CREATEROLE", dbtest.CreateLoginRole(t, rs, "anbud_test_o_createrole", "CREATEROLE"), true},
		{"a member of a role with CREATEDB", func() string {
			dbtest.CreateLoginRole(t, rs, "anbud_test_og_createdb", "CREATEDB")
			return dbtest.CreateLoginRole(t, rs, "anbud_test_o_createdb", "", "anbud_test_og_createdb")
		}(), true},
		{"a member of a predefined role that runs programs", dbtest.CreateLoginRole(t, rs, "anbud_test_o_program", "", "pg_execute_server_program"), true},
		{"a member of the owner role with nothing else", dbtest.CreateLoginRole(t, rs, "anbud_test_o_member", "", dbtest.RoleMigrator), false},
	} {
		t.Run(c.name, func(t *testing.T) {
			pool := dbtest.Pool(t, c.dsn)

			verdict, err := db.CheckOwnerRole(ctx, pool, remoteDSN)
			if c.refused != errors.Is(err, db.ErrOwnerTooPrivileged) || (err != nil && !c.refused) {
				t.Errorf("remote: got (%v, %v), want a refusal = %v", verdict, err, c.refused)
			}
			verdict, err = db.CheckOwnerRole(ctx, pool, c.dsn)
			switch {
			case c.refused && (err != nil || verdict != db.RoleWarn):
				t.Errorf("loopback: got (%v, %v), want (RoleWarn, nil)", verdict, err)
			case !c.refused && (err != nil || verdict != db.RoleOK):
				t.Errorf("loopback: got (%v, %v), want (RoleOK, nil)", verdict, err)
			}
		})
	}
}

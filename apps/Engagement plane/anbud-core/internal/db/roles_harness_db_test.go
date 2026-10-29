package db_test

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"runtime"
	"strings"
	"sync"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// The role helpers of dbtest create the three roles of scripts/roles.sql with fixed names,
// set new passwords on two of them and drop them again, so they may only ever do that on a
// throwaway cluster. These tests run the helpers against a server that already holds the
// roles, the way a shared cluster with a real anbud_core would, and show that the helpers
// refuse before they change anything. Needs a throwaway server: run with make test-db.

// fakeTB stands in for a *testing.T so that a helper that ends its test with t.Fatal or
// t.Skip can be run, and what it did read, without ending the test that runs it. The
// embedded interface is nil: any method the helpers were not expected to call panics,
// which fails the test loudly.
type fakeTB struct {
	testing.TB
	mu       sync.Mutex
	cleanups []func()
	fatal    string
	skipped  string
	errs     []string
	logs     []string
}

func (f *fakeTB) Helper() {}

func (f *fakeTB) Cleanup(fn func()) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.cleanups = append(f.cleanups, fn)
}

func (f *fakeTB) Fatal(args ...any) {
	f.mu.Lock()
	f.fatal = fmt.Sprint(args...)
	f.mu.Unlock()
	runtime.Goexit()
}

func (f *fakeTB) Fatalf(format string, args ...any) {
	f.mu.Lock()
	f.fatal = fmt.Sprintf(format, args...)
	f.mu.Unlock()
	runtime.Goexit()
}

func (f *fakeTB) Skip(args ...any) {
	f.mu.Lock()
	f.skipped = fmt.Sprint(args...)
	f.mu.Unlock()
	runtime.Goexit()
}

func (f *fakeTB) Errorf(format string, args ...any) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.errs = append(f.errs, fmt.Sprintf(format, args...))
}

func (f *fakeTB) Logf(format string, args ...any) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.logs = append(f.logs, fmt.Sprintf(format, args...))
}

// run calls fn in a goroutine of its own, so that the runtime.Goexit behind Fatal and Skip
// ends only that goroutine, then runs the cleanups last in, first out, as testing does.
func (f *fakeTB) run(fn func(testing.TB)) {
	done := make(chan struct{})
	go func() {
		defer close(done)
		fn(f)
	}()
	<-done
	for {
		f.mu.Lock()
		if len(f.cleanups) == 0 {
			f.mu.Unlock()
			return
		}
		last := len(f.cleanups) - 1
		cleanup := f.cleanups[last]
		f.cleanups = f.cleanups[:last]
		f.mu.Unlock()
		cleanup()
	}
}

var roleModelRoles = []string{dbtest.RoleMigrator, dbtest.RoleApp, dbtest.RoleDBA}

const rolesByName = `('anbud_migrator', 'anbud_app', 'anbud_dba')`

// bareServer returns a pool on the throwaway server's own database, after making sure that
// none of the roles exists yet, and drops whatever roles exist when the test ends. A test that
// finds a role there refuses, exactly as the helpers do: it is not its to touch.
func bareServer(t *testing.T) (*pgxpool.Pool, string) {
	t.Helper()
	server := dbtest.ServerDSN(t)
	ctx := roleCtx(t)
	admin := dbtest.Pool(t, server)
	if n := scanInt(ctx, t, admin, `SELECT count(*) FROM pg_roles WHERE rolname IN `+rolesByName); n != 0 {
		const msg = "a role of the role model already exists on this server; this test arranges the roles itself and needs a cluster without them"
		if os.Getenv(dbtest.RequireVar) == "1" {
			t.Fatal(msg)
		}
		t.Skip(msg)
	}
	cfg, err := pgx.ParseConfig(server)
	if err != nil {
		t.Fatal("unparsable server DSN")
	}
	// Registered after the pool, so it runs before the pool closes.
	t.Cleanup(func() {
		for _, role := range []string{dbtest.RoleApp, dbtest.RoleDBA, dbtest.RoleMigrator} {
			_, _ = admin.Exec(context.Background(), `DROP ROLE IF EXISTS `+pgx.Identifier{role}.Sanitize())
		}
	})
	return admin, cfg.Database
}

// rolesDigest summarises every attribute of the three roles that a helper could change,
// the password hash included, as one hash: a failure message must not print the rows,
// which hold the hash.
func rolesDigest(ctx context.Context, t *testing.T, admin *pgxpool.Pool) string {
	t.Helper()
	rows := scanStrings(ctx, t, admin, `
		SELECT r.rolname || ' ' || r.rolcanlogin || ' ' || r.rolconnlimit || ' ' || coalesce(a.rolpassword, '-') || ' '
		       || r.rolsuper || r.rolcreatedb || r.rolcreaterole || r.rolbypassrls || r.rolreplication || ' '
		       || coalesce(array_to_string(r.rolconfig, ','), '-')
		  FROM pg_roles r JOIN pg_authid a ON a.oid = r.oid
		 WHERE r.rolname IN `+rolesByName+` ORDER BY r.rolname`)
	sum := sha256.Sum256([]byte(strings.Join(rows, "\n")))
	return fmt.Sprintf("%d roles, digest %s", len(rows), hex.EncodeToString(sum[:]))
}

func hexSecret(t *testing.T) string {
	t.Helper()
	var b [12]byte
	if _, err := rand.Read(b[:]); err != nil {
		t.Fatal("random secret")
	}
	return hex.EncodeToString(b[:])
}

// When the roles exist and the test did not create them, RoleDatabase and RolesOn refuse:
// with ANBUD_REQUIRE_DB_TESTS=1 the test fails, otherwise it skips, and either way the
// refusal comes before the script runs and before a password is set, so every attribute of
// every role, its password hash included, is what it was, and no database was made.
func TestRoleHelpersRefuseRolesTheyDidNotCreateAndChangeNothing(t *testing.T) {
	admin, _ := bareServer(t)
	ctx := roleCtx(t)

	// The roles as a real cluster might hold them: with passwords and limits no helper sets.
	for i, role := range roleModelRoles {
		login := "LOGIN"
		if role == dbtest.RoleDBA {
			login = "NOLOGIN"
		}
		stmt := fmt.Sprintf("CREATE ROLE %s %s PASSWORD '%s' CONNECTION LIMIT %d", pgx.Identifier{role}.Sanitize(), login, hexSecret(t), 7+i)
		if _, err := admin.Exec(ctx, stmt); err != nil {
			t.Fatalf("arrange a role: %s", dbtest.Describe(err))
		}
	}
	before := rolesDigest(ctx, t, admin)
	if !strings.HasPrefix(before, "3 roles") {
		t.Fatalf("arranged %s, want 3 roles", before)
	}
	databasesBefore := scanInt(ctx, t, admin, `SELECT count(*) FROM pg_database WHERE datname LIKE 'anbud\_test\_%'`)

	helpers := map[string]func(testing.TB){
		"RoleDatabase": func(tb testing.TB) { dbtest.RoleDatabase(tb) },
		// The database named here is never made: the refusal comes first.
		"RolesOn": func(tb testing.TB) {
			dbtest.RolesOn(tb, "postgres://u:never-used@127.0.0.1:1/anbud_test_never_made?sslmode=disable")
		},
	}
	for name, helper := range helpers {
		t.Run(name+" fails when ANBUD_REQUIRE_DB_TESTS is 1", func(t *testing.T) {
			t.Setenv(dbtest.RequireVar, "1")
			f := &fakeTB{}
			f.run(helper)
			if !strings.Contains(f.fatal, "already exists") || !strings.Contains(f.fatal, "make test-db") {
				t.Errorf("the helper ended with fatal %q and skip %q, want the refusal that points at make test-db", f.fatal, f.skipped)
			}
			if f.skipped != "" {
				t.Errorf("the helper skipped (%q) although the run requires database tests", f.skipped)
			}
			if len(f.errs) != 0 {
				t.Errorf("the helper reported errors while refusing: %q", f.errs)
			}
		})
		t.Run(name+" skips when it is not required", func(t *testing.T) {
			t.Setenv(dbtest.RequireVar, "")
			f := &fakeTB{}
			f.run(helper)
			if !strings.Contains(f.skipped, "already exists") || !strings.Contains(f.skipped, "make test-db") {
				t.Errorf("the helper ended with skip %q and fatal %q, want the refusal that points at make test-db", f.skipped, f.fatal)
			}
			if f.fatal != "" {
				t.Errorf("the helper failed (%q) although database tests are not required", f.fatal)
			}
		})
	}

	if after := rolesDigest(ctx, t, admin); after != before {
		t.Errorf("a helper changed the roles it did not create: before %s, after %s", before, after)
	}
	if got := scanInt(ctx, t, admin, `SELECT count(*) FROM pg_database WHERE datname LIKE 'anbud\_test\_%'`); got != databasesBefore {
		t.Errorf("a helper left %d throwaway databases, want the %d there were", got, databasesBefore)
	}
}

// A role that cannot be dropped at the end, because something outside the test still depends
// on it, must not vanish from sight: it is logged, and it is also why the next role test on
// that server refuses. Here a privilege on a database that outlives the test is the thing
// that depends on a role.
func TestARoleLeftBehindIsLoggedAndNotSilent(t *testing.T) {
	admin, serverDatabase := bareServer(t)
	ctx := roleCtx(t)
	// Registered after bareServer's cleanup, so it runs before it: the grant goes first,
	// and then bareServer's cleanup can drop the role.
	t.Cleanup(func() {
		_, _ = admin.Exec(context.Background(), `REVOKE ALL ON DATABASE `+pgx.Identifier{serverDatabase}.Sanitize()+` FROM anbud_app`)
	})

	f := &fakeTB{}
	f.run(func(tb testing.TB) {
		dbtest.RoleDatabase(tb)
		grant := `GRANT CONNECT ON DATABASE ` + pgx.Identifier{serverDatabase}.Sanitize() + ` TO anbud_app`
		if _, err := admin.Exec(ctx, grant); err != nil {
			tb.Fatalf("arrange a dependency: %s", dbtest.Describe(err))
		}
	})

	if f.fatal != "" || f.skipped != "" {
		t.Fatalf("the helper ended early: fatal %q, skip %q", f.fatal, f.skipped)
	}
	logged := strings.Join(f.logs, "\n")
	if !strings.Contains(logged, "anbud_app") || !strings.Contains(logged, "left in place") {
		t.Errorf("the role that could not be dropped was not logged; the helper logged %q", logged)
	}
	if len(f.errs) != 0 {
		t.Errorf("the helper reported errors for a role it was right to leave: %q", f.errs)
	}
	if n := scanInt(ctx, t, admin, `SELECT count(*) FROM pg_roles WHERE rolname = 'anbud_app'`); n != 1 {
		t.Errorf("anbud_app was dropped although something depends on it (%d rows)", n)
	}
}

// A role an experiment made may end up owning things and holding privileges: a view, the
// schema, the database, CONNECT. The helper still removes it, and the roles of the model
// with it, so the next test starts on a cluster with none of them.
func TestATestRoleThatOwnsThingsIsStillDropped(t *testing.T) {
	admin, _ := bareServer(t)
	ctx := roleCtx(t)

	f := &fakeTB{}
	f.run(func(tb testing.TB) {
		rs := dbtest.RoleDatabase(tb)
		dbtest.CreateLoginRole(tb, rs, "anbud_test_owner_of_things", "", dbtest.RoleDBA)
		db, err := pgx.Connect(ctx, rs.AdminDSN)
		if err != nil {
			tb.Fatalf("connect: %s", dbtest.Describe(err))
		}
		defer func() { _ = db.Close(context.Background()) }()
		for _, stmt := range []string{
			`CREATE VIEW public.anbud_test_owned_view AS SELECT 1 AS one`,
			`ALTER VIEW public.anbud_test_owned_view OWNER TO anbud_test_owner_of_things`,
			`ALTER SCHEMA public OWNER TO anbud_test_owner_of_things`,
			`ALTER DATABASE ` + pgx.Identifier{rs.Database}.Sanitize() + ` OWNER TO anbud_test_owner_of_things`,
		} {
			if _, err := db.Exec(ctx, stmt); err != nil {
				tb.Fatalf("arrange %q: %s", stmt, dbtest.Describe(err))
			}
		}
	})

	if f.fatal != "" || f.skipped != "" {
		t.Fatalf("the helpers ended early: fatal %q, skip %q", f.fatal, f.skipped)
	}
	if len(f.errs) != 0 {
		t.Errorf("cleaning up reported errors: %q", f.errs)
	}
	if n := scanInt(ctx, t, admin, `SELECT count(*) FROM pg_roles WHERE rolname = 'anbud_test_owner_of_things' OR rolname IN `+rolesByName); n != 0 {
		t.Errorf("%d roles are still there after the cleanup, want none", n)
	}
}

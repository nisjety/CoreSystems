package dbtest

// Roles for database tests. scripts/roles.sql creates three roles with fixed names:
// roles belong to the cluster, not to a database, so a test cannot give them
// names of its own. The helpers here create them only on the throwaway server, by
// running the script itself on a database of the test's own, give the two login roles
// random passwords for the test, and drop them when the test ends.
//
// A cluster that already holds one of the three roles is not a throwaway: a real
// anbud_core may use them, and resetting a password or dropping a role would break it.
// So a helper that finds one refuses before it changes anything (a failure when
// ANBUD_REQUIRE_DB_TESTS is 1, a skip otherwise), and never touches a role it did not
// create. scripts/test-db.sh starts a fresh container for every run, so a normal run has
// none to find.
//
// Everything that connects goes through the same guard as the rest of dbtest: the
// database must be postgres or start with anbud_test. Nothing here prints a DSN, a
// role name taken from a DSN, a password or a driver error, because those carry
// them; Describe says only what is safe.

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

// The roles scripts/roles.sql creates.
const (
	RoleMigrator = "anbud_migrator"
	RoleApp      = "anbud_app"
	RoleDBA      = "anbud_dba"
)

const (
	// roleLockKey serialises every test that creates or drops the roles. An advisory
	// lock is per database and the roles are per cluster, so every holder takes it on
	// the server's own database, the one ANBUD_TEST_DSN names.
	roleLockKey     int64 = 0x616e627564726f6c // "anbudrol"
	roleLockTimeout       = 5 * time.Minute
	passwordBytes         = 24
)

// RoleSet is a throwaway database prepared by scripts/roles.sql, with a DSN for the
// server's own role (a superuser, for looking and arranging) and one for each login role.
//
// The DSNs carry passwords. A test that logs, prints or formats a RoleSet by accident
// (%v, %+v, %#v, t.Logf("%v", rs)) must not put them in the output, so String and
// GoString say the database name and nothing else. Reaching a DSN takes a field name.
type RoleSet struct {
	Database    string // the throwaway database
	AdminDSN    string // the test server's role, on this database
	MigratorDSN string // anbud_migrator, which owns the database and its objects
	AppDSN      string // anbud_app
}

// String is what fmt prints for a RoleSet (%v, %+v and %s): the database name only.
func (rs RoleSet) String() string { return "dbtest.RoleSet{Database: " + rs.Database + "}" }

// GoString is what fmt prints for %#v: the same, never the DSNs.
func (rs RoleSet) GoString() string { return rs.String() }

// RoleDatabase creates a fresh, unmigrated database and prepares it as a DBA would: it
// runs scripts/roles.sql on it, then sets random passwords on the two login roles.
// The database is dropped when the test ends, and so are the roles, if this test
// created them. A test uses one of RoleDatabase and RolesOn for its first database.
func RoleDatabase(t testing.TB) RoleSet {
	t.Helper()
	server := ServerDSN(t)
	return prepareRoles(t, server, newDatabaseName(t), true)
}

// RolesOn prepares an existing database, the way a DBA prepares one that was
// migrated before the role model existed. It needs a database dbtest made for the
// test, such as the one Database returns, and refuses the server's own.
func RolesOn(t testing.TB, databaseDSN string) RoleSet {
	t.Helper()
	server := ServerDSN(t)
	if err := CheckDSN(databaseDSN); err != nil {
		t.Fatal(err)
	}
	cfg, err := pgx.ParseConfig(databaseDSN)
	if err != nil {
		t.Fatal("dbtest: refusing an unparsable DSN")
	}
	if cfg.Database == allowedDatabase {
		t.Fatal("dbtest: RolesOn needs a throwaway database, not the server's own")
	}
	return prepareRoles(t, server, cfg.Database, false)
}

// roleSession is one test's hold on the roles: the lock that keeps other tests from
// creating or dropping them meanwhile, and whether the roles are this test's to drop.
// They are only when none of the three was there when the session began: every role
// the test meets later is then one it made.
type roleSession struct {
	lock *pgx.Conn
	owns bool // set once the session has seen that no role existed; finish drops them
}

var sessions sync.Map // testing.TB -> *roleSession

// errRolesExist is the refusal when a role of the role model already exists on the
// server. The text is fixed: it names the roles, which are the model's own and fixed,
// and nothing taken from a DSN.
const errRolesExist = "dbtest: a role named anbud_migrator, anbud_app or anbud_dba already exists on this server, and the role tests create these roles with new passwords and drop them again, which would break a real database that uses them; run these tests on a throwaway cluster only (make test-db)"

// roleStart is what a test does when it asks for the roles.
type roleStart int

const (
	// startCreate: none of the three roles exists, so this test creates them and drops
	// them when it ends.
	startCreate roleStart = iota
	// startSkip: a role exists that this test did not create, and ANBUD_REQUIRE_DB_TESTS
	// is not 1, so the test skips, as it does when there is no server at all.
	startSkip
	// startFail: the same, with ANBUD_REQUIRE_DB_TESTS=1, so the run must not pass by
	// skipping.
	startFail
)

// decideRoleStart decides what a test does, given how many of the three roles exist
// already and the value of ANBUD_REQUIRE_DB_TESTS. A role that exists was not created by
// this test, because the test holds the role lock and has not made any yet, so it is a
// real one, or one a crashed run left behind. Either way the test must not touch it:
// resetting its password or dropping it is not the test's to do.
func decideRoleStart(existing int, require string) roleStart {
	switch {
	case existing == 0:
		return startCreate
	case unsetDSNFails(require):
		return startFail
	default:
		return startSkip
	}
}

func sessionFor(ctx context.Context, t testing.TB, server string) *roleSession {
	t.Helper()
	if s, ok := sessions.Load(t); ok {
		return s.(*roleSession)
	}
	lockCtx, cancel := context.WithTimeout(ctx, roleLockTimeout)
	defer cancel()
	conn := mustConnect(lockCtx, t, server)
	if _, err := conn.Exec(lockCtx, `SELECT pg_advisory_lock($1)`, roleLockKey); err != nil {
		_ = conn.Close(context.Background())
		t.Fatalf("dbtest: could not take the role lock: %s", Describe(err))
	}
	s := &roleSession{lock: conn}
	sessions.Store(t, s)
	// Registered before anything the test creates, so it runs after all of it: it
	// drops the roles last, when no database of the test is left, and lets go of
	// the lock after that. Closing the session releases the advisory lock.
	t.Cleanup(func() { s.finish(t) })

	var existing int
	if err := conn.QueryRow(ctx, `SELECT count(*) FROM pg_roles WHERE rolname IN ($1, $2, $3)`, RoleMigrator, RoleApp, RoleDBA).Scan(&existing); err != nil {
		t.Fatalf("dbtest: could not look for the roles: %s", Describe(err))
	}
	// Before anything is created, and before any password is set: a refusal here has
	// changed nothing on the server.
	switch decideRoleStart(existing, os.Getenv(RequireVar)) {
	case startFail:
		t.Fatal(errRolesExist)
	case startSkip:
		t.Skip(errRolesExist)
	}
	s.owns = true
	return s
}

func (s *roleSession) finish(t testing.TB) {
	t.Helper()
	sessions.Delete(t)
	ctx, cancel := context.WithTimeout(context.Background(), cleanupTimeout)
	defer cancel()
	// Nothing to drop when the session was refused: the roles it found are not its own.
	// A session that was not refused created all three, or only some of them (a failed
	// script may stop part way), and DROP ROLE IF EXISTS covers both.
	if s.owns {
		for _, role := range []string{RoleApp, RoleDBA, RoleMigrator} {
			left, err := dropRole(ctx, s.lock, role)
			switch {
			case err != nil:
				t.Errorf("dbtest: drop a role: %s", Describe(err))
			case left:
				t.Logf("dbtest: the role %s was left in place: another database still depends on it, so the next role test on this server will refuse to start", role)
			}
		}
	}
	_ = s.lock.Close(context.Background())
}

// dropRole drops a role. A role that another database still depends on is left in
// place and reported through left: it is not this test's to force, but a role left behind
// must not be invisible, and the next role test refuses to start while it exists.
func dropRole(ctx context.Context, conn *pgx.Conn, role string) (left bool, err error) {
	_, err = conn.Exec(ctx, "DROP ROLE IF EXISTS "+pgx.Identifier{role}.Sanitize())
	if SQLState(err) == "2BP01" { // dependent_objects_still_exist
		return true, nil
	}
	return false, err
}

func prepareRoles(t testing.TB, server, name string, create bool) RoleSet {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), roleLockTimeout+setupTimeout)
	defer cancel()

	sessionFor(ctx, t, server)
	if create {
		createDatabase(ctx, t, server, name)
	}
	// Runs before the session's cleanup, which drops the roles: a database that still
	// depends on a role would keep it.
	t.Cleanup(func() { dropDatabase(t, server, name) })

	admin := dsnForDatabase(t, server, name)
	rs := RoleSet{Database: name, AdminDSN: admin}
	runRolesScript(ctx, t, rs)

	conn := mustConnect(ctx, t, admin)
	defer func() { _ = conn.Close(context.Background()) }()
	migratorPassword, appPassword := randomPassword(t), randomPassword(t)
	setPassword(ctx, t, conn, RoleMigrator, migratorPassword)
	setPassword(ctx, t, conn, RoleApp, appPassword)
	rs.MigratorDSN = roleDSN(t, server, RoleMigrator, migratorPassword, name)
	rs.AppDSN = roleDSN(t, server, RoleApp, appPassword, name)
	return rs
}

// RunRolesScript runs scripts/roles.sql on the set's database again, as the server's role.
// The script is idempotent, so this changes nothing but what an earlier change undid.
func RunRolesScript(t testing.TB, rs RoleSet) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), setupTimeout)
	defer cancel()
	runRolesScript(ctx, t, rs)
}

// TryRolesScriptOnServerDatabase runs scripts/roles.sql, as the server's role, for the
// database ANBUD_TEST_DSN itself names (postgres, on the throwaway container), and
// returns what the server answered. That name does not start with anbud, so the script
// must refuse it before it changes anything: the test that uses this says so. It takes the
// role lock like every role helper, so no other test makes the roles meanwhile, and it
// skips when the server's own database does start with anbud, where the script would not
// refuse and would prepare it.
func TryRolesScriptOnServerDatabase(t testing.TB) error {
	t.Helper()
	server := ServerDSN(t)
	cfg, err := pgx.ParseConfig(server)
	if err != nil {
		t.Fatal("dbtest: refusing an unparsable DSN")
	}
	if strings.HasPrefix(cfg.Database, "anbud") {
		t.Skip("dbtest: the server's own database starts with anbud, so roles.sql would not refuse it")
	}
	ctx, cancel := context.WithTimeout(context.Background(), roleLockTimeout+setupTimeout)
	defer cancel()
	sessionFor(ctx, t, server)
	conn := mustConnect(ctx, t, server)
	defer func() { _ = conn.Close(context.Background()) }()
	_, err = conn.Exec(ctx, ExpandRolesScript(ReadRolesScript(t), cfg.Database))
	return err
}

func runRolesScript(ctx context.Context, t testing.TB, rs RoleSet) {
	t.Helper()
	script := ExpandRolesScript(ReadRolesScript(t), rs.Database)
	conn := mustConnect(ctx, t, rs.AdminDSN)
	defer func() { _ = conn.Close(context.Background()) }()
	// One query with several statements goes over the simple protocol, which is how
	// the script runs, whole, in the transaction it opens itself.
	if _, err := conn.Exec(ctx, script); err != nil {
		t.Fatalf("dbtest: scripts/roles.sql failed: %s", Describe(err))
	}
}

func setPassword(ctx context.Context, t testing.TB, conn *pgx.Conn, role, password string) {
	t.Helper()
	// The password is hex, so it needs no quoting beyond the literal form.
	stmt := "ALTER ROLE " + pgx.Identifier{role}.Sanitize() + " PASSWORD '" + password + "'"
	if _, err := conn.Exec(ctx, stmt); err != nil {
		t.Fatalf("dbtest: could not set a password: %s", Describe(err))
	}
}

// CreateLoginRole adds a login role to the cluster for one experiment: it gets a random
// password, the one attribute attr (CREATEROLE, CREATEDB, BYPASSRLS or none), CONNECT on
// rs's database (scripts/roles.sql takes it from PUBLIC, so a role the script has not
// heard of cannot connect without it), and membership of the given roles. It returns the
// DSN for rs's database and drops the role when the test ends, after taking back
// anything the experiment gave it. name must start with anbud_test_.
func CreateLoginRole(t testing.TB, rs RoleSet, name, attr string, memberOf ...string) string {
	t.Helper()
	return createLoginRole(t, rs, name, attr, true, memberOf)
}

// CreateLoginRoleWithoutConnect is CreateLoginRole for a role that holds nothing at all:
// no CONNECT on the database, no attribute, no group. It is the role a test uses to show
// that PUBLIC no longer lets an unknown role in.
func CreateLoginRoleWithoutConnect(t testing.TB, rs RoleSet, name string) string {
	t.Helper()
	return createLoginRole(t, rs, name, "", false, nil)
}

func createLoginRole(t testing.TB, rs RoleSet, name, attr string, connectGrant bool, memberOf []string) string {
	t.Helper()
	if !validTestRoleName(name) {
		t.Fatal("dbtest: a test role name must be lower case and start with anbud_test_")
	}
	if !validLoginAttribute(attr) {
		t.Fatal("dbtest: a test role may carry CREATEROLE, CREATEDB or BYPASSRLS, and nothing else")
	}
	server := ServerDSN(t)
	ctx, cancel := context.WithTimeout(context.Background(), setupTimeout)
	defer cancel()
	conn := mustConnect(ctx, t, rs.AdminDSN)
	defer func() { _ = conn.Close(context.Background()) }()

	password := randomPassword(t)
	ident := pgx.Identifier{name}.Sanitize()
	stmt := "CREATE ROLE " + ident + " LOGIN " + attr + " PASSWORD '" + password + "'"
	if _, err := conn.Exec(ctx, stmt); err != nil {
		t.Fatalf("dbtest: could not create a test role: %s", Describe(err))
	}
	t.Cleanup(func() { dropTestRole(t, rs, name) })
	if connectGrant {
		grant := "GRANT CONNECT ON DATABASE " + pgx.Identifier{rs.Database}.Sanitize() + " TO " + ident
		if _, err := conn.Exec(ctx, grant); err != nil {
			t.Fatalf("dbtest: could not let a test role connect: %s", Describe(err))
		}
	}
	for _, group := range memberOf {
		if _, err := conn.Exec(ctx, "GRANT "+pgx.Identifier{group}.Sanitize()+" TO "+ident); err != nil {
			t.Fatalf("dbtest: could not add a test role to a group: %s", Describe(err))
		}
	}
	return roleDSN(t, server, name, password, rs.Database)
}

// dropTestRole removes a role an experiment made. DROP ROLE refuses a role that still owns
// an object or holds a privilege in any database, and an experiment may have given it
// both: a view or the schema or the database to own, CONNECT on the database. So what it owns in this database, and
// any database it owns, goes to the server's role first (REASSIGN OWNED), then what it was
// granted is taken back (DROP OWNED, after the reassignment it drops nothing), then the
// role goes. A role that survives anyway is reported, never silent.
func dropTestRole(t testing.TB, rs RoleSet, name string) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), cleanupTimeout)
	defer cancel()
	conn, err := connect(ctx, rs.AdminDSN)
	if err != nil {
		t.Errorf("dbtest: could not connect to drop a test role: %s", Describe(err))
		return
	}
	defer func() { _ = conn.Close(context.Background()) }()

	ident := pgx.Identifier{name}.Sanitize()
	var exists bool
	if err := conn.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)`, name).Scan(&exists); err != nil {
		t.Errorf("dbtest: look for a test role: %s", Describe(err))
		return
	}
	if exists {
		// A session that has just closed is still being torn down by the server, which drops
		// its temporary tables as its backend exits. A temporary table of the role that is
		// half gone makes REASSIGN OWNED fail with XX000 ("could not open relation"), so
		// wait until the role has no backend left, and try again if the server still says so.
		waitForNoSessions(ctx, conn, name)
		for _, stmt := range []string{"REASSIGN OWNED BY " + ident + " TO CURRENT_USER", "DROP OWNED BY " + ident} {
			if err := execRetrying(ctx, conn, stmt); err != nil {
				t.Errorf("dbtest: clear what a test role owns or holds: %s", Describe(err))
				return
			}
		}
	}
	left, err := dropRole(ctx, conn, name)
	switch {
	case err != nil:
		t.Errorf("dbtest: drop a test role: %s", Describe(err))
	case left:
		t.Errorf("dbtest: a test role was left in place: something still depends on it, so a later test would fail to create it again")
	}
}

// waitForNoSessions waits, for a few seconds at most, until no backend of the role is left on the
// server. It returns either way: a session that stays is the test's own to close, and the
// statements that follow say what it caused.
func waitForNoSessions(ctx context.Context, conn *pgx.Conn, role string) {
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		var n int
		if err := conn.QueryRow(ctx, `SELECT count(*) FROM pg_stat_activity WHERE usename = $1`, role).Scan(&n); err != nil || n == 0 {
			return
		}
		time.Sleep(50 * time.Millisecond)
	}
}

// execRetrying runs a statement and runs it again, a few times, when the server answers
// XX000 (internal_error), which is what it says about a relation that another backend is
// dropping at that moment. Any other error ends it at once.
func execRetrying(ctx context.Context, conn *pgx.Conn, stmt string) error {
	var err error
	for range 20 {
		if _, err = conn.Exec(ctx, stmt); err == nil || SQLState(err) != "XX000" {
			return err
		}
		time.Sleep(100 * time.Millisecond)
	}
	return err
}

var testRoleName = regexp.MustCompile(`^anbud_test_[a-z0-9_]+$`)

func validTestRoleName(name string) bool { return testRoleName.MatchString(name) }

func validLoginAttribute(attr string) bool {
	switch attr {
	case "", "CREATEROLE", "CREATEDB", "BYPASSRLS":
		return true
	}
	return false
}

// MustBeThrowaway fails t unless every DSN names a database the guard allows. A test
// that hands a DSN to a command, which opens its own connection, calls it first, so
// the guard covers those connections as well as the ones the helpers open.
func MustBeThrowaway(t testing.TB, dsns ...string) {
	t.Helper()
	if err := checkAll(dsns); err != nil {
		t.Fatal(err)
	}
}

func checkAll(dsns []string) error {
	for _, dsn := range dsns {
		if dsn == "" {
			continue
		}
		if err := CheckDSN(dsn); err != nil {
			return err
		}
	}
	return nil
}

// Pool opens a pool on dsn after the guard has passed it, and closes it when the
// test ends.
func Pool(t testing.TB, dsn string) *pgxpool.Pool {
	t.Helper()
	if err := CheckDSN(dsn); err != nil {
		t.Fatal(err)
	}
	cfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		t.Fatal("dbtest: refusing an unparsable DSN")
	}
	cfg.MaxConns = maxConns
	pool, err := pgxpool.NewWithConfig(context.Background(), cfg)
	if err != nil {
		t.Fatalf("dbtest: open pool: %s", Describe(err))
	}
	t.Cleanup(pool.Close)
	return pool
}

// connect opens one connection, after the guard has passed the DSN. Its errors say
// no more than Describe does.
func connect(ctx context.Context, dsn string) (*pgx.Conn, error) {
	if err := CheckDSN(dsn); err != nil {
		return nil, err
	}
	conn, err := pgx.Connect(ctx, dsn)
	if err != nil {
		return nil, fmt.Errorf("dbtest: could not connect (%s)", Describe(err))
	}
	return conn, nil
}

func mustConnect(ctx context.Context, t testing.TB, dsn string) *pgx.Conn {
	t.Helper()
	conn, err := connect(ctx, dsn)
	if err != nil {
		t.Fatal(err)
	}
	return conn
}

// roleDSN is the server's DSN with the role, its password and the database
// swapped in. It keeps the host and the query, so the guard and the TLS rule see
// what they saw.
func roleDSN(t testing.TB, server, role, password, database string) string {
	t.Helper()
	u, err := url.Parse(server)
	if err != nil || (u.Scheme != "postgres" && u.Scheme != "postgresql") {
		t.Fatalf("dbtest: %s must be a postgres:// URL for this helper", EnvVar)
	}
	u.User = url.UserPassword(role, password)
	u.Path = "/" + database
	return u.String()
}

func randomPassword(t testing.TB) string {
	t.Helper()
	var b [passwordBytes]byte
	if _, err := rand.Read(b[:]); err != nil {
		t.Fatalf("dbtest: random password: %v", err)
	}
	return hex.EncodeToString(b[:])
}

// RolesScriptPath is where scripts/roles.sql is, found from this source file or, when
// the build has no source paths, by walking up from the working directory to the
// module root. It is empty when it cannot be found.
func RolesScriptPath() string {
	const rel = "scripts/roles.sql"
	if _, file, _, ok := runtime.Caller(0); ok {
		p := filepath.Join(filepath.Dir(file), "..", "..", "..", filepath.FromSlash(rel))
		if _, err := os.Stat(p); err == nil {
			return p
		}
	}
	dir, err := os.Getwd()
	if err != nil {
		return ""
	}
	for range 8 {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			p := filepath.Join(dir, filepath.FromSlash(rel))
			if _, err := os.Stat(p); err == nil {
				return p
			}
			return ""
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	return ""
}

// ReadRolesScript returns the text of scripts/roles.sql and fails t when it cannot.
func ReadRolesScript(t testing.TB) string {
	t.Helper()
	path := RolesScriptPath()
	if path == "" {
		t.Fatal("dbtest: scripts/roles.sql was not found from the module root")
	}
	script, err := os.ReadFile(path)
	if err != nil {
		t.Fatal("dbtest: scripts/roles.sql could not be read")
	}
	return string(script)
}

// ExpandRolesScript expands the two psql variable references the script uses,
// :'dbname' as a literal and :"dbname" as an identifier, the way psql -v dbname=...
// does. The script uses nothing else of psql, so pgx can run what comes out; a test
// of the script keeps it so, and scripts/test-db.sh runs it under the real psql.
func ExpandRolesScript(script, database string) string {
	ident := pgx.Identifier{database}.Sanitize()
	literal := "'" + strings.ReplaceAll(database, "'", "''") + "'"
	return strings.NewReplacer(`:"dbname"`, ident, `:'dbname'`, literal).Replace(script)
}

// Describe says how an error ended, in words that carry no part of a DSN. A server
// error gives its SQLSTATE and, except for an authentication error (class 28), whose
// message names the role, and an unknown database (class 3D), whose message names
// the database, the server's message, which names objects and never the connection.
// Any other error gives its type only: a driver error quotes the host, the role and
// the database.
func Describe(err error) string {
	if err == nil {
		return "no error"
	}
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		switch {
		case strings.HasPrefix(pgErr.Code, "28"):
			return "SQLSTATE " + pgErr.Code + " (authentication)"
		case strings.HasPrefix(pgErr.Code, "3D"):
			return "SQLSTATE " + pgErr.Code + " (catalog name)"
		}
		return "SQLSTATE " + pgErr.Code + ": " + pgErr.Message
	}
	if strings.HasPrefix(err.Error(), "dbtest: ") {
		return err.Error() // one of this package's own messages, already clean
	}
	return fmt.Sprintf("error of type %T", err)
}

// SQLState returns the SQLSTATE of a server error, or "" for no error or an error
// that is not the server's.
func SQLState(err error) string {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		return pgErr.Code
	}
	return ""
}

// Outcome says how a statement ended in one word: "ok", the SQLSTATE of the server's
// error, or "error" for a failure that is not the server's, which must never read
// as a refusal or as a success.
func Outcome(err error) string {
	if err == nil {
		return "ok"
	}
	if code := SQLState(err); code != "" {
		return code
	}
	return "error"
}

// DSNParts lists what must never appear in anything a command prints: the whole
// DSN, the role, the password, the host with its port and the database. A part
// that is empty is left out, since every output contains it.
func DSNParts(dsn string) []string {
	parts := []string{dsn}
	if u, err := url.Parse(dsn); err == nil {
		parts = append(parts, u.User.Username(), u.Host, strings.TrimPrefix(u.Path, "/"))
		if pw, ok := u.User.Password(); ok {
			parts = append(parts, pw)
		}
	}
	out := parts[:0]
	for _, p := range parts {
		if p != "" {
			out = append(out, p)
		}
	}
	return out
}

package dbtest

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"regexp"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgconn"
)

// The role helpers prepare a throwaway database the way scripts/roles.sql prepares
// a real one. These tests cover the parts that need no server: how the script's
// psql variables are expanded, how a role's DSN is built, and what the helpers
// refuse to print or to connect to.

func TestExpandRolesScriptSubstitutesBothForms(t *testing.T) {
	const script = `SELECT set_config('x', :'dbname', true);
ALTER DATABASE :"dbname" OWNER TO anbud_migrator;
GRANT CONNECT ON DATABASE :"dbname" TO anbud_app; -- not :dbname, and a cast ::text stays
`
	got := ExpandRolesScript(script, "anbud_test_ab12cd34")
	want := `SELECT set_config('x', 'anbud_test_ab12cd34', true);
ALTER DATABASE "anbud_test_ab12cd34" OWNER TO anbud_migrator;
GRANT CONNECT ON DATABASE "anbud_test_ab12cd34" TO anbud_app; -- not :dbname, and a cast ::text stays
`
	if got != want {
		t.Errorf("expanded script =\n%s\nwant\n%s", got, want)
	}
}

// The name is the helper's own, hex with a prefix, so it needs no quoting. The
// expansion quotes anyway, so a name with a quote in it cannot end a literal or an
// identifier early.
func TestExpandRolesScriptQuotesAHostileName(t *testing.T) {
	got := ExpandRolesScript(`:'dbname' :"dbname"`, `a'b"c`)
	if got != `'a''b"c' "a'b""c"` {
		t.Errorf("expansion = %s", got)
	}
}

func TestRoleDSNKeepsTheServerAndSwapsRoleAndDatabase(t *testing.T) {
	const server = "postgres://postgres:server-secret@127.0.0.1:55432/postgres?sslmode=disable"
	dsn := roleDSN(t, server, RoleApp, "app-secret", "anbud_test_ab12cd34")

	u, err := url.Parse(dsn)
	if err != nil {
		t.Fatalf("the role DSN does not parse")
	}
	pw, _ := u.User.Password()
	switch {
	case u.Scheme != "postgres", u.Host != "127.0.0.1:55432", u.Path != "/anbud_test_ab12cd34",
		u.User.Username() != RoleApp, pw != "app-secret", u.Query().Get("sslmode") != "disable":
		t.Errorf("the role DSN has the wrong parts: scheme %q host %q path %q user %q sslmode %q",
			u.Scheme, u.Host, u.Path, u.User.Username(), u.Query().Get("sslmode"))
	}
	if strings.Contains(dsn, "server-secret") {
		t.Error("the role DSN carries the server's password")
	}
	if err := CheckDSN(dsn); err != nil {
		t.Errorf("the guard refuses the role DSN: %v", err)
	}
}

// Every DSN the helpers build names a throwaway database, so the guard accepts it;
// one that names anything else is refused before any connection is opened.
func TestConnectRefusesADSNTheGuardRefusesWithoutDialling(t *testing.T) {
	for _, dsn := range []string{
		"postgres://u:guard-pw@127.0.0.1:1/anbud_core?sslmode=disable",
		"postgres://u:guard-pw@127.0.0.1:1/anbud?sslmode=disable",
	} {
		conn, err := connect(context.Background(), dsn)
		if conn != nil {
			t.Fatal("connect returned a connection for a DSN the guard refuses")
		}
		if !errors.Is(err, errRefused) {
			t.Errorf("error = %v, want the guard's refusal", err)
		}
	}
}

// A connection or server error can quote the host, the role and the database, and
// an authentication error names the role. The helpers print only the SQLSTATE, and
// the server's message when it names objects rather than the connection.
func TestDescribeNeverEchoesAConnectionError(t *testing.T) {
	const quoted = "failed to connect to `host=db.example.com user=anbud_app database=anbud_test_x`: dial error"
	cases := []struct {
		name    string
		err     error
		mustHas string
		leaks   []string
	}{
		{"a driver error", errors.New(quoted), "*errors.errorString", []string{"db.example.com", "anbud_app", "anbud_test_x"}},
		{"an authentication error", &pgconn.PgError{Code: "28P01", Message: `password authentication failed for user "anbud_app"`}, "28P01", []string{"anbud_app", "password authentication"}},
		{"an unknown database", &pgconn.PgError{Code: "3D000", Message: `database "anbud_test_x" does not exist`}, "3D000", []string{"anbud_test_x", "does not exist"}},
		{"a permission error keeps its message", &pgconn.PgError{Code: "42501", Message: "permission denied for table raw_documents"}, "permission denied for table raw_documents", nil},
		{"no error", nil, "no error", nil},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got := Describe(c.err)
			if !strings.Contains(got, c.mustHas) {
				t.Errorf("Describe = %q, want it to contain %q", got, c.mustHas)
			}
			for _, leak := range c.leaks {
				if strings.Contains(got, leak) {
					t.Errorf("Describe = %q, which carries %q", got, leak)
				}
			}
		})
	}
}

// Outcome says how a statement ended in one word a test can compare: "ok", the
// server's SQLSTATE, or "error" for a failure that is not the server's, such as a
// dropped connection, which must never read as a success or as a refusal.
func TestOutcomeNamesHowAStatementEnded(t *testing.T) {
	cases := []struct {
		name string
		err  error
		want string
	}{
		{"success", nil, "ok"},
		{"a refusal", &pgconn.PgError{Code: "42501"}, "42501"},
		{"a refusal inside another error", errors.Join(errors.New("context"), &pgconn.PgError{Code: "55000"}), "55000"},
		{"a failure that is not the server's", errors.New("connection reset"), "error"},
	}
	for _, c := range cases {
		if got := Outcome(c.err); got != c.want {
			t.Errorf("%s: Outcome = %q, want %q", c.name, got, c.want)
		}
	}
}

func TestSQLStateReadsTheCodeOfAServerError(t *testing.T) {
	wrapped := errors.Join(errors.New("context"), &pgconn.PgError{Code: "42501"})
	if got := SQLState(wrapped); got != "42501" {
		t.Errorf("SQLState = %q, want 42501", got)
	}
	if got := SQLState(errors.New("no code here")); got != "" {
		t.Errorf("SQLState = %q, want none", got)
	}
	if got := SQLState(nil); got != "" {
		t.Errorf("SQLState(nil) = %q, want none", got)
	}
}

// DSNParts lists what must never appear in anything a command prints: the whole
// DSN, the role, the password, the host and the database.
func TestDSNPartsListsEveryPartThatMustStayOutOfOutput(t *testing.T) {
	const dsn = "postgres://anbud_app:part-secret@127.0.0.1:55432/anbud_test_ab12?sslmode=disable"
	parts := DSNParts(dsn)
	for _, want := range []string{dsn, "anbud_app", "part-secret", "127.0.0.1:55432", "anbud_test_ab12"} {
		found := false
		for _, p := range parts {
			found = found || p == want
		}
		if !found {
			t.Errorf("DSNParts omits %q: %v", want, parts)
		}
	}
	for _, p := range parts {
		if p == "" {
			t.Error("DSNParts returned an empty part, which every output contains")
		}
	}
}

func TestRandomPasswordIsLongHexAndNeverRepeats(t *testing.T) {
	a, b := randomPassword(t), randomPassword(t)
	if a == b {
		t.Error("two passwords are equal")
	}
	if !regexp.MustCompile(`^[0-9a-f]{48}$`).MatchString(a) {
		t.Errorf("the password is not 48 hex digits (length %d)", len(a))
	}
}

// A login role the tests add for one experiment may carry one of these attributes
// and no other, so a test cannot build a statement out of a string it was given.
func TestLoginRoleAttributesAreAllowListed(t *testing.T) {
	for _, ok := range []string{"", "CREATEROLE", "CREATEDB", "BYPASSRLS"} {
		if !validLoginAttribute(ok) {
			t.Errorf("%q should be allowed", ok)
		}
	}
	for _, bad := range []string{"SUPERUSER; DROP DATABASE postgres", "superuser", "CREATEROLE CREATEDB", "REPLICATION"} {
		if validLoginAttribute(bad) {
			t.Errorf("%q should be refused", bad)
		}
	}
	if !validTestRoleName("anbud_test_flag_1") {
		t.Error("a plain test role name was refused")
	}
	for _, bad := range []string{"postgres", "anbud_app", "anbud_test_x; DROP ROLE y", "ANBUD_TEST_X", "anbud_test_"} {
		if validTestRoleName(bad) {
			t.Errorf("%q should be refused as a test role name", bad)
		}
	}
}

// A test that gives a command a DSN has the guard check it first: any DSN that names
// a database the guard refuses fails the whole set, and an empty one (a variable the
// test leaves unset) is skipped.
func TestCheckAllAppliesTheGuardToEveryDSN(t *testing.T) {
	good := "postgres://u:guard-pw@127.0.0.1:5432/anbud_test_ab12?sslmode=disable"
	admin := "postgres://u:guard-pw@127.0.0.1:5432/postgres?sslmode=disable"
	bad := "postgres://u:guard-pw@127.0.0.1:5432/anbud_core?sslmode=disable"
	if err := checkAll([]string{good, admin, ""}); err != nil {
		t.Errorf("checkAll refused throwaway DSNs: %v", err)
	}
	if err := checkAll([]string{good, bad}); !errors.Is(err, errRefused) {
		t.Errorf("checkAll(good, bad) = %v, want the guard's refusal", err)
	}
	if err := checkAll([]string{bad}); !errors.Is(err, errRefused) {
		t.Errorf("checkAll(bad) = %v, want the guard's refusal", err)
	}
}

// A RoleSet holds DSNs with passwords in plain strings, and a test that logs or prints one by
// accident must not put them in its output. fmt uses String for %v, %+v and %s and
// GoString for %#v, through a pointer and through an exported field of another struct.
func TestRoleSetNeverPrintsItsDSNs(t *testing.T) {
	rs := RoleSet{
		Database:    "anbud_test_ab12cd34",
		AdminDSN:    "postgres://postgres:admin-secret@127.0.0.1:5432/anbud_test_ab12cd34?sslmode=disable",
		MigratorDSN: "postgres://anbud_migrator:migrator-secret@127.0.0.1:5432/anbud_test_ab12cd34?sslmode=disable",
		AppDSN:      "postgres://anbud_app:app-secret@127.0.0.1:5432/anbud_test_ab12cd34?sslmode=disable",
	}
	wrapped := struct{ Set RoleSet }{rs}
	outputs := map[string]string{
		"%v":  fmt.Sprintf("%v", rs),
		"%+v": fmt.Sprintf("%+v", rs),
		"%#v": fmt.Sprintf("%#v", rs),
		//lint:ignore S1025 the %s verb is what this entry tests, not a conversion to string
		"%s":                  fmt.Sprintf("%s", rs),
		"Sprint":              fmt.Sprint(rs),
		"a pointer, %+v":      fmt.Sprintf("%+v", &rs),
		"a pointer, %#v":      fmt.Sprintf("%#v", &rs),
		"a field, %+v":        fmt.Sprintf("%+v", wrapped),
		"a field, %#v":        fmt.Sprintf("%#v", wrapped),
		"a slice, %v":         fmt.Sprintf("%v", []RoleSet{rs}),
		"a map value, %+v":    fmt.Sprintf("%+v", map[string]RoleSet{"set": rs}),
		"String and GoString": rs.String() + rs.GoString(),
	}
	for name, got := range outputs {
		for _, leak := range []string{"admin-secret", "migrator-secret", "app-secret", "postgres://", "127.0.0.1", "sslmode", "anbud_migrator:", "anbud_app:"} {
			if strings.Contains(got, leak) {
				t.Errorf("%s prints %q: %s", name, leak, got)
			}
		}
		if !strings.Contains(got, rs.Database) {
			t.Errorf("%s does not say which database the set is for: %s", name, got)
		}
	}
}

// What a role test does when it asks for the roles depends on whether any of them exists
// already, and on ANBUD_REQUIRE_DB_TESTS, which is the same switch the server DSN uses: only
// the value 1 makes the refusal a failure.
func TestDecideRoleStart(t *testing.T) {
	cases := []struct {
		name     string
		existing int
		require  string
		want     roleStart
	}{
		{"no role exists", 0, "", startCreate},
		{"no role exists, require set", 0, "1", startCreate},
		{"one role exists, require set", 1, "1", startFail},
		{"all three exist, require set", 3, "1", startFail},
		{"one role exists, require unset", 1, "", startSkip},
		{"two roles exist, require 0", 2, "0", startSkip},
		{"one role exists, require true is not 1", 1, "true", startSkip},
	}
	for _, c := range cases {
		if got := decideRoleStart(c.existing, c.require); got != c.want {
			t.Errorf("%s: decideRoleStart(%d, %q) = %d, want %d", c.name, c.existing, c.require, got, c.want)
		}
	}
}

// The refusal is fixed text: it names the roles of the model, which are fixed, says what to
// do, and carries nothing from a DSN.
func TestRolesExistRefusalIsFixedText(t *testing.T) {
	for _, want := range []string{"anbud_migrator", "anbud_app", "anbud_dba", "throwaway", "make test-db"} {
		if !strings.Contains(errRolesExist, want) {
			t.Errorf("the refusal does not mention %q: %s", want, errRolesExist)
		}
	}
	for _, leak := range []string{"postgres://", "password=", "@"} {
		if strings.Contains(errRolesExist, leak) {
			t.Errorf("the refusal carries %q", leak)
		}
	}
	if strings.ContainsAny(errRolesExist, "\r\n") {
		t.Error("the refusal spans several lines")
	}
}

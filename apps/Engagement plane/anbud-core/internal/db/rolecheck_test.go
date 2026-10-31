package db_test

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"regexp"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/anbud/internal/db"
)

// The runtime-role check (O2): the role of the pool the engine writes through must
// not be able to drop the append-only triggers. These tests are pure. A fake
// stands in for the database, so the decision runs without Postgres. The same
// check against real roles is in roles_check_db_test.go and rolecheck_reasons_db_test.go.

// factsRow is what the role query returns: one boolean per field of db.RoleFacts, in the
// order of the fields, which is the order db.ReadRoleFacts scans them in.
type factsRow struct {
	vals []bool
	err  error
}

func (r factsRow) Scan(dest ...any) error {
	if r.err != nil {
		return r.err
	}
	if len(dest) != len(r.vals) {
		return fmt.Errorf("fake scan: %d destinations for %d values", len(dest), len(r.vals))
	}
	for i, d := range dest {
		p, ok := d.(*bool)
		if !ok {
			return fmt.Errorf("fake scan: destination %d is %T, want *bool", i, d)
		}
		*p = r.vals[i]
	}
	return nil
}

// fakeQuerier is the query seam. It returns one canned row and records the query.
type fakeQuerier struct {
	row   factsRow
	calls int
	sql   string
}

func (q *fakeQuerier) QueryRow(_ context.Context, sql string, _ ...any) pgx.Row {
	q.calls++
	q.sql = sql
	return q.row
}

// factCount is how many facts a role has: the fields of db.RoleFacts, every one of them a
// bool. A fact added later is picked up by every test below that loops over the fields.
func factCount(t testing.TB) int {
	t.Helper()
	typ := reflect.TypeFor[db.RoleFacts]()
	for i := range typ.NumField() {
		if typ.Field(i).Type.Kind() != reflect.Bool {
			t.Fatalf("db.RoleFacts.%s is not a bool", typ.Field(i).Name)
		}
	}
	return typ.NumField()
}

// onlyFact returns facts with the field at index i true and every other false.
func onlyFact(i int) db.RoleFacts {
	var f db.RoleFacts
	reflect.ValueOf(&f).Elem().Field(i).SetBool(true)
	return f
}

// factsFromMask returns the facts whose bit is set in mask: bit i is field i.
func factsFromMask(mask int) db.RoleFacts {
	var f db.RoleFacts
	v := reflect.ValueOf(&f).Elem()
	for i := range v.NumField() {
		v.Field(i).SetBool(mask&(1<<i) != 0)
	}
	return f
}

func fieldName(i int) string { return reflect.TypeFor[db.RoleFacts]().Field(i).Name }

// zeroFacts is a fake that reads back a role with no fact.
func zeroFacts(t testing.TB) *fakeQuerier {
	t.Helper()
	return &fakeQuerier{row: factsRow{vals: make([]bool, factCount(t))}}
}

// withFacts is a fake that reads back the given facts.
func withFacts(t testing.TB, f db.RoleFacts) *fakeQuerier {
	t.Helper()
	vals := make([]bool, factCount(t))
	v := reflect.ValueOf(f)
	for i := range vals {
		vals[i] = v.Field(i).Bool()
	}
	return &fakeQuerier{row: factsRow{vals: vals}}
}

// writesWord finds a keyword of a statement that changes something, as a whole word, so the
// column names rolcreaterole and rolcreatedb do not count.
var writesWord = regexp.MustCompile(`\b(insert|update|delete|alter|grant|revoke|create|drop|truncate)\b`)

const (
	loopbackDSN = "postgres://role-user:role-marker-pw@127.0.0.1:5432/role_db?sslmode=disable"
	remoteDSN   = "postgres://role-user:role-marker-pw@db.example.com:5432/role_db?sslmode=verify-full"
)

// ReadRoleFacts scans one column into each field, in the order of the fields, and a
// column of the query that maps to the wrong field would make an unrelated fact true. This
// is checked here for every field; the query itself is checked against a real server.
func TestReadRoleFactsMapsEachColumnToItsField(t *testing.T) {
	t.Parallel()

	n := factCount(t)
	if n < 11 {
		t.Fatalf("db.RoleFacts has %d fields; the reasons the role check must know are at least 11", n)
	}
	for i := range n {
		t.Run(fieldName(i), func(t *testing.T) {
			t.Parallel()

			vals := make([]bool, n)
			vals[i] = true
			got, err := db.ReadRoleFacts(context.Background(), &fakeQuerier{row: factsRow{vals: vals}})
			if err != nil {
				t.Fatalf("ReadRoleFacts: %v", err)
			}
			if want := onlyFact(i); got != want {
				t.Errorf("with only column %d true, facts = %+v, want %+v", i, got, want)
			}
		})
	}
}

// The role query only reads the catalog, asks about the role the pool uses and about what
// that role can become, and names every reason the decision table has a field for.
func TestReadRoleFactsAsksAboutTheCurrentRoleOnly(t *testing.T) {
	t.Parallel()

	q := zeroFacts(t)
	if _, err := db.ReadRoleFacts(context.Background(), q); err != nil {
		t.Fatalf("ReadRoleFacts: %v", err)
	}
	sql := strings.ToLower(q.sql)
	if !strings.HasPrefix(strings.TrimSpace(sql), "with") && !strings.HasPrefix(strings.TrimSpace(sql), "select") {
		t.Errorf("the role query is not a read: %q", q.sql)
	}
	for _, want := range []string{
		"current_user", "pg_roles", "rolsuper", "rolcreaterole", "rolcreatedb", "rolbypassrls",
		// membership, not only inheritance: a role that does not inherit can still SET ROLE
		"pg_has_role", "'member'",
		// what it owns
		"pg_class", "relowner", "pg_proc", "proowner", "pg_namespace", "nspowner", "current_schemas", "pg_database", "datdba", "current_database",
		// the roles of the model, and the predefined roles that cross the boundary
		"anbud_migrator", "anbud_dba",
		"pg_execute_server_program", "pg_write_server_files", "pg_read_server_files", "pg_write_all_data", "pg_signal_backend", "pg_maintain", "pg_checkpoint",
	} {
		if !strings.Contains(sql, want) {
			t.Errorf("the role query does not mention %s: %q", want, q.sql)
		}
	}
	if write := writesWord.FindString(sql); write != "" {
		t.Errorf("the role query mentions %q: %q", write, q.sql)
	}
	if strings.Contains(sql, "'usage'") {
		t.Errorf("the role query asks for USAGE, which a role that does not inherit lacks although it can SET ROLE: %q", q.sql)
	}
}

// Each fact is enough on its own, and a role with none of them has nothing to answer for.
// The loop is over the fields, so a fact added to db.RoleFacts that Excessive forgets
// fails here.
func TestEveryFactAloneMakesTheRoleExcessive(t *testing.T) {
	t.Parallel()

	n := factCount(t)
	if (db.RoleFacts{}).Excessive() {
		t.Error("a role with no fact is excessive")
	}
	for i := range n {
		f := onlyFact(i)
		if !f.Excessive() {
			t.Errorf("a role with only %s is not excessive", fieldName(i))
		}
		if verdict, err := db.DecideRole(f, false); err != nil || verdict != db.RoleWarn {
			t.Errorf("only %s on loopback: got (%v, %v), want (RoleWarn, nil)", fieldName(i), verdict, err)
		}
		if _, err := db.DecideRole(f, true); !errors.Is(err, db.ErrRoleTooPrivileged) {
			t.Errorf("only %s on a remote host: error = %v, want ErrRoleTooPrivileged", fieldName(i), err)
		}
	}
}

// The decision table: any one fact makes a role too privileged. On a loopback host
// that is a warning, on any other host a refusal. A role with no fact passes anywhere.
// Every combination of facts, on both kinds of host.
func TestDecideRoleFullTable(t *testing.T) {
	t.Parallel()

	n := factCount(t)
	for mask := range 1 << n {
		f := factsFromMask(mask)
		for _, remote := range []bool{false, true} {
			name := fmt.Sprintf("%+v remote=%v", f, remote)
			verdict, err := db.DecideRole(f, remote)
			switch {
			case mask == 0:
				if verdict != db.RoleOK || err != nil {
					t.Errorf("%s: got (%v, %v), want (RoleOK, nil)", name, verdict, err)
				}
			case !remote:
				if verdict != db.RoleWarn || err != nil {
					t.Errorf("%s: got (%v, %v), want (RoleWarn, nil)", name, verdict, err)
				}
			default:
				if !errors.Is(err, db.ErrRoleTooPrivileged) {
					t.Errorf("%s: error = %v, want ErrRoleTooPrivileged", name, err)
				}
			}
			if f.Excessive() != (mask != 0) {
				t.Errorf("%s: Excessive() = %v", name, f.Excessive())
			}
		}
	}
}

func TestDecideRoleNamedCases(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name    string
		facts   db.RoleFacts
		remote  bool
		verdict db.RoleVerdict
		refused bool
	}{
		{"superuser on a remote host is refused", db.RoleFacts{Superuser: true}, true, db.RoleOK, true},
		{"superuser on loopback warns", db.RoleFacts{Superuser: true}, false, db.RoleWarn, false},
		{"CREATEROLE alone is too much on a remote host", db.RoleFacts{CreateRole: true}, true, db.RoleOK, true},
		{"CREATEDB alone is too much on a remote host", db.RoleFacts{CreateDB: true}, true, db.RoleOK, true},
		{"BYPASSRLS alone is too much on a remote host", db.RoleFacts{BypassRLS: true}, true, db.RoleOK, true},
		{"a predefined role that reaches files or programs is too much on a remote host", db.RoleFacts{HoldsPredefinedRole: true}, true, db.RoleOK, true},
		{"membership of the owner role is too much on a remote host", db.RoleFacts{HoldsMigratorRole: true}, true, db.RoleOK, true},
		{"membership of the DBA group is too much on a remote host", db.RoleFacts{HoldsDBARole: true}, true, db.RoleOK, true},
		{"owning a relation alone is too much on a remote host", db.RoleFacts{OwnsRelation: true}, true, db.RoleOK, true},
		{"owning a function alone is too much on a remote host", db.RoleFacts{OwnsFunction: true}, true, db.RoleOK, true},
		{"owning the schema alone is too much on a remote host", db.RoleFacts{OwnsSchema: true}, true, db.RoleOK, true},
		{"owning the database alone is too much on a remote host", db.RoleFacts{OwnsDatabase: true}, true, db.RoleOK, true},
		{"owning a relation on loopback warns", db.RoleFacts{OwnsRelation: true}, false, db.RoleWarn, false},
		{"owning a function on loopback warns", db.RoleFacts{OwnsFunction: true}, false, db.RoleWarn, false},
		{"membership of the DBA group on loopback warns", db.RoleFacts{HoldsDBARole: true}, false, db.RoleWarn, false},
		{"a least-privilege role passes on a remote host", db.RoleFacts{}, true, db.RoleOK, false},
		{"a least-privilege role passes on loopback", db.RoleFacts{}, false, db.RoleOK, false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			t.Parallel()

			verdict, err := db.DecideRole(c.facts, c.remote)
			if c.refused {
				if !errors.Is(err, db.ErrRoleTooPrivileged) {
					t.Fatalf("error = %v, want ErrRoleTooPrivileged", err)
				}
				return
			}
			if err != nil || verdict != c.verdict {
				t.Fatalf("got (%v, %v), want (%v, nil)", verdict, err, c.verdict)
			}
		})
	}
}

// The role that owns the schema is judged by the same facts but for ownership: it owns
// everything by design, and it is the owner role, and the roles of the model are its to
// hold. What still counts is what lets a role escape the database: a superuser, CREATEROLE,
// CREATEDB, BYPASSRLS and a predefined role that reaches files, programs or other sessions.
func TestDecideOwnerRoleIgnoresOwnershipAndTheModelsOwnRoles(t *testing.T) {
	t.Parallel()

	counts := map[string]bool{
		"Superuser": true, "CreateRole": true, "CreateDB": true, "BypassRLS": true, "HoldsPredefinedRole": true,
		"HoldsMigratorRole": false, "HoldsDBARole": false,
		"OwnsRelation": false, "OwnsFunction": false, "OwnsSchema": false, "OwnsDatabase": false,
	}
	n := factCount(t)
	if len(counts) != n {
		t.Fatalf("the table above names %d facts and db.RoleFacts has %d: a new fact must be classified for the owner role", len(counts), n)
	}
	for i := range n {
		name := fieldName(i)
		want, known := counts[name]
		if !known {
			t.Fatalf("the fact %s is not classified for the owner role", name)
		}
		f := onlyFact(i)
		if got := f.ExcessiveAsOwner(); got != want {
			t.Errorf("only %s: ExcessiveAsOwner() = %v, want %v", name, got, want)
		}
		_, err := db.DecideOwnerRole(f, true)
		if want != errors.Is(err, db.ErrOwnerTooPrivileged) {
			t.Errorf("only %s on a remote host: error = %v, want a refusal = %v", name, err, want)
		}
		if err != nil && !errors.Is(err, db.ErrOwnerTooPrivileged) {
			t.Errorf("only %s: error = %v, want ErrOwnerTooPrivileged or none", name, err)
		}
		verdict, err := db.DecideOwnerRole(f, false)
		if err != nil || (want && verdict != db.RoleWarn) || (!want && verdict != db.RoleOK) {
			t.Errorf("only %s on loopback: got (%v, %v), want warn = %v and no error", name, verdict, err, want)
		}
	}

	// The real owner role holds the model's owner role and owns the objects, the schema and
	// the database: that is no reason to refuse it.
	owner := db.RoleFacts{HoldsMigratorRole: true, OwnsRelation: true, OwnsFunction: true, OwnsSchema: true, OwnsDatabase: true}
	if verdict, err := db.DecideOwnerRole(owner, true); err != nil || verdict != db.RoleOK {
		t.Errorf("the owner role on a remote host: got (%v, %v), want (RoleOK, nil)", verdict, err)
	}
	// A superuser or a role with CREATEROLE is refused as the owner whatever else it holds.
	for _, f := range []db.RoleFacts{{Superuser: true, OwnsRelation: true}, {CreateRole: true, HoldsMigratorRole: true}} {
		if _, err := db.DecideOwnerRole(f, true); !errors.Is(err, db.ErrOwnerTooPrivileged) {
			t.Errorf("%+v as the owner on a remote host: error = %v, want ErrOwnerTooPrivileged", f, err)
		}
	}
}

// The owner check is a different refusal from the runtime check, so that a start can say which
// DSN is wrong, and neither is mistaken for the other.
func TestOwnerRefusalIsNotTheRuntimeRefusal(t *testing.T) {
	t.Parallel()

	if errors.Is(db.ErrOwnerTooPrivileged, db.ErrRoleTooPrivileged) || errors.Is(db.ErrRoleTooPrivileged, db.ErrOwnerTooPrivileged) {
		t.Error("the owner refusal and the runtime refusal are the same error")
	}
	if errors.Is(db.ErrOwnerCheckFailed, db.ErrRoleCheckFailed) || errors.Is(db.ErrRoleCheckFailed, db.ErrOwnerCheckFailed) {
		t.Error("the owner check failure and the runtime check failure are the same error")
	}
}

// CheckRuntimeRole judges the host with the rule CheckTLS applies: localhost,
// 127.0.0.1, ::1 and a Unix socket directory are local, and a remote host anywhere
// in the DSN, a fallback included, makes the host remote. The owner check judges it the
// same way, with the same function.
func TestCheckRuntimeRoleJudgesTheHostLikeCheckTLS(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name       string
		dsn        string
		wantRemote bool
	}{
		{"127.0.0.1", "postgres://u:role-marker-pw@127.0.0.1:5432/d?sslmode=disable", false},
		{"localhost", "postgres://u:role-marker-pw@localhost:5432/d?sslmode=disable", false},
		{"::1", "postgres://u:role-marker-pw@[::1]:5432/d?sslmode=disable", false},
		{"a Unix socket directory", "host=/var/run/postgresql dbname=d user=u password=role-marker-pw", false},
		{"a remote host", remoteDSN, true},
		{"the keyword form with a remote host", "host=db.example.com port=5432 user=u password=role-marker-pw dbname=d sslmode=verify-full", true},
		{"a remote fallback behind a loopback host", "host=127.0.0.1,db.example.com user=u password=role-marker-pw dbname=d sslmode=disable", true},
		{"a DSN that does not parse is treated as remote", "postgres://u:role-marker-pw@db.example.com:notaport/d", true},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			t.Parallel()

			// A superuser: too privileged for the runtime role and for the owner role alike.
			super := withFacts(t, db.RoleFacts{Superuser: true})
			verdict, err := db.CheckRuntimeRole(context.Background(), super, c.dsn)
			if c.wantRemote {
				if !errors.Is(err, db.ErrRoleTooPrivileged) {
					t.Fatalf("a superuser on a remote host: error = %v, want ErrRoleTooPrivileged", err)
				}
			} else if err != nil || verdict != db.RoleWarn {
				t.Fatalf("a superuser on loopback: got (%v, %v), want (RoleWarn, nil)", verdict, err)
			}
			verdict, err = db.CheckOwnerRole(context.Background(), withFacts(t, db.RoleFacts{Superuser: true}), c.dsn)
			if c.wantRemote {
				if !errors.Is(err, db.ErrOwnerTooPrivileged) {
					t.Fatalf("a superuser owner on a remote host: error = %v, want ErrOwnerTooPrivileged", err)
				}
			} else if err != nil || verdict != db.RoleWarn {
				t.Fatalf("a superuser owner on loopback: got (%v, %v), want (RoleWarn, nil)", verdict, err)
			}

			clean, err := db.CheckRuntimeRole(context.Background(), zeroFacts(t), c.dsn)
			if err != nil || clean != db.RoleOK {
				t.Fatalf("a least-privilege role: got (%v, %v), want (RoleOK, nil)", clean, err)
			}
			clean, err = db.CheckOwnerRole(context.Background(), zeroFacts(t), c.dsn)
			if err != nil || clean != db.RoleOK {
				t.Fatalf("an owner role with no fact: got (%v, %v), want (RoleOK, nil)", clean, err)
			}
		})
	}
}

// A query that fails cannot be read as "fine". The check fails closed, with a fixed
// error: the driver's own text can quote connection details.
func TestCheckRuntimeRoleFailsClosedWhenTheQueryFails(t *testing.T) {
	t.Parallel()

	for _, dsn := range []string{loopbackDSN, remoteDSN} {
		q := &fakeQuerier{row: factsRow{err: errors.New("driver text with role-marker-pw and db.example.com")}}
		verdict, err := db.CheckRuntimeRole(context.Background(), q, dsn)
		if !errors.Is(err, db.ErrRoleCheckFailed) {
			t.Fatalf("error = %v, want ErrRoleCheckFailed", err)
		}
		if verdict != db.RoleOK {
			t.Errorf("verdict = %v with an error, want the zero verdict", verdict)
		}
		for _, leak := range []string{"role-marker-pw", "db.example.com", "driver text"} {
			if strings.Contains(err.Error(), leak) {
				t.Errorf("the error echoes %q: %v", leak, err)
			}
		}

		q = &fakeQuerier{row: factsRow{err: errors.New("driver text with role-marker-pw and db.example.com")}}
		verdict, err = db.CheckOwnerRole(context.Background(), q, dsn)
		if !errors.Is(err, db.ErrOwnerCheckFailed) {
			t.Fatalf("owner: error = %v, want ErrOwnerCheckFailed", err)
		}
		if verdict != db.RoleOK {
			t.Errorf("owner: verdict = %v with an error, want the zero verdict", verdict)
		}
		for _, leak := range []string{"role-marker-pw", "db.example.com", "driver text"} {
			if strings.Contains(err.Error(), leak) {
				t.Errorf("the owner error echoes %q: %v", leak, err)
			}
		}
	}
}

func TestCheckRuntimeRoleQueriesOnce(t *testing.T) {
	t.Parallel()

	q := zeroFacts(t)
	if _, err := db.CheckRuntimeRole(context.Background(), q, loopbackDSN); err != nil {
		t.Fatalf("CheckRuntimeRole: %v", err)
	}
	if q.calls != 1 {
		t.Errorf("the check ran %d queries, want 1", q.calls)
	}
	o := zeroFacts(t)
	if _, err := db.CheckOwnerRole(context.Background(), o, loopbackDSN); err != nil {
		t.Fatalf("CheckOwnerRole: %v", err)
	}
	if o.calls != 1 {
		t.Errorf("the owner check ran %d queries, want 1", o.calls)
	}
}

// Every message is fixed text. None carries a part of any DSN (role, password, host,
// database), none names a role of the model, and the warning is one line.
func TestRoleMessagesAreFixedText(t *testing.T) {
	t.Parallel()

	texts := map[string]string{
		"refusal":       db.ErrRoleTooPrivileged.Error(),
		"failure":       db.ErrRoleCheckFailed.Error(),
		"warning":       db.RoleWarning,
		"owner refusal": db.ErrOwnerTooPrivileged.Error(),
		"owner failure": db.ErrOwnerCheckFailed.Error(),
		"owner warning": db.OwnerWarning,
	}
	variable := map[string]string{
		"refusal": "ANBUD_DSN", "failure": "ANBUD_DSN", "warning": "ANBUD_DSN",
		"owner refusal": "ANBUD_MIGRATE_DSN", "owner failure": "ANBUD_MIGRATE_DSN", "owner warning": "ANBUD_MIGRATE_DSN",
	}
	for name, text := range texts {
		if strings.TrimSpace(text) == "" {
			t.Errorf("%s is empty", name)
		}
		if strings.ContainsAny(text, "\r\n") {
			t.Errorf("%s spans several lines: %q", name, text)
		}
		for _, leak := range []string{"postgres://", "@", "role-marker-pw", "db.example.com", "password=", "sslmode", "anbud_"} {
			if strings.Contains(text, leak) {
				t.Errorf("%s carries %q: %q", name, leak, text)
			}
		}
		if !strings.Contains(text, variable[name]) {
			t.Errorf("%s does not name %s: %q", name, variable[name], text)
		}
	}
	if !strings.Contains(texts["refusal"], "ANBUD_MIGRATE_DSN") {
		t.Errorf("the refusal does not say where migrations go: %q", texts["refusal"])
	}
	for _, name := range []string{"refusal", "warning", "owner refusal", "owner warning"} {
		if !strings.Contains(texts[name], "docs/ROLES.md") {
			t.Errorf("the %s does not point at docs/ROLES.md: %q", name, texts[name])
		}
	}
	// What the runtime messages say is wrong has to cover what the check looks at.
	for _, name := range []string{"refusal", "warning"} {
		for _, reason := range []string{"superuser", "CREATEROLE", "CREATEDB", "BYPASSRLS", "member", "own"} {
			if !strings.Contains(texts[name], reason) {
				t.Errorf("the %s does not mention %q: %q", name, reason, texts[name])
			}
		}
	}
	// The messages of the two checks differ, so a start says which of the two DSNs is wrong.
	if texts["refusal"] == texts["owner refusal"] || texts["warning"] == texts["owner warning"] {
		t.Error("the runtime and the owner checks print the same message")
	}
}

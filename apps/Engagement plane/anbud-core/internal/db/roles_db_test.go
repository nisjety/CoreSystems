package db_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

// Needs a throwaway server on ANBUD_TEST_DSN: run these with make test-db. Each test
// builds a fresh database, runs scripts/roles.sql on it and connects as the roles it
// creates, so the privileges under test are the ones the script grants, and a
// failure is the server's refusal and not a trigger's. The roles belong to the
// cluster, so the helpers create them with their fixed names, only on the test
// server, and drop them when the test ends.

const roleTestTimeout = 90 * time.Second

func roleCtx(t *testing.T) context.Context {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), roleTestTimeout)
	t.Cleanup(cancel)
	return ctx
}

// migratedRoles prepares a database with the roles, migrates it as the migrator the
// way a deployment does, and returns it with a pool for the server's own role (the
// superuser of the throwaway cluster), which the tests use to look and to arrange.
func migratedRoles(t *testing.T) (dbtest.RoleSet, *pgxpool.Pool) {
	t.Helper()
	rs := dbtest.RoleDatabase(t)
	migrator := dbtest.Pool(t, rs.MigratorDSN)
	if err := db.ApplyMigrations(roleCtx(t), migrator); err != nil {
		t.Fatalf("ApplyMigrations as the migrator: %s", dbtest.Describe(err))
	}
	return rs, dbtest.Pool(t, rs.AdminDSN)
}

// outcome runs a statement and says how it ended: "ok", the SQLSTATE of the server's
// error, or "error" for a failure that is not the server's.
func outcome(ctx context.Context, pool *pgxpool.Pool, sql string, args ...any) string {
	_, err := pool.Exec(ctx, sql, args...)
	return dbtest.Outcome(err)
}

func scanStrings(ctx context.Context, t *testing.T, pool *pgxpool.Pool, sql string, args ...any) []string {
	t.Helper()
	rows, err := pool.Query(ctx, sql, args...)
	if err != nil {
		t.Fatalf("query: %s", dbtest.Describe(err))
	}
	defer rows.Close()
	var out []string
	for rows.Next() {
		var s string
		if err := rows.Scan(&s); err != nil {
			t.Fatalf("scan: %s", dbtest.Describe(err))
		}
		out = append(out, s)
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("rows: %s", dbtest.Describe(err))
	}
	return out
}

func scanBool(ctx context.Context, t *testing.T, pool *pgxpool.Pool, sql string, args ...any) bool {
	t.Helper()
	var b bool
	if err := pool.QueryRow(ctx, sql, args...).Scan(&b); err != nil {
		t.Fatalf("query: %s", dbtest.Describe(err))
	}
	return b
}

var (
	evidenceTables = []string{"raw_search_pages", "raw_documents", "download_attempts"}
	ledgerTables   = []string{"sync_runs", "sync_days"}
	// allTables ends with the two relations migration 0006 adds, the tombstones and the
	// view of the notices that still hold their body. The tests that take the first five
	// (the tables the application writes) leave them out.
	allTables = []string{"raw_search_pages", "raw_documents", "download_attempts", "sync_runs", "sync_days", "schema_migrations", "raw_redactions", "raw_documents_live"}

	tablePrivileges = []string{"SELECT", "INSERT", "UPDATE", "DELETE", "TRUNCATE", "REFERENCES", "TRIGGER"}
)

// expectedMatrix is the privilege matrix of the role model, table by table. The
// migrator owns every table and holds every privilege on it, so it is not listed.
// The application reads the tombstones and the view and writes neither: the default
// privileges would hand it INSERT on the new table, and migration 0006 takes that back.
func expectedMatrix() map[string]map[string][]string {
	app := map[string][]string{
		"raw_search_pages":   {"SELECT", "INSERT"},
		"raw_documents":      {"SELECT", "INSERT"},
		"download_attempts":  {"SELECT", "INSERT"},
		"sync_runs":          {"SELECT", "INSERT", "UPDATE"},
		"sync_days":          {"SELECT", "INSERT", "UPDATE"},
		"schema_migrations":  {},
		"raw_redactions":     {"SELECT"},
		"raw_documents_live": {"SELECT"},
	}
	dba := map[string][]string{}
	for _, table := range allTables {
		dba[table] = []string{"SELECT"}
	}
	return map[string]map[string][]string{dbtest.RoleApp: app, dbtest.RoleDBA: dba}
}

// grantedMatrix reads the matrix from the server, for every role and table.
func grantedMatrix(ctx context.Context, t *testing.T, admin *pgxpool.Pool) map[string]map[string][]string {
	t.Helper()
	got := map[string]map[string][]string{}
	for _, role := range []string{dbtest.RoleApp, dbtest.RoleDBA} {
		got[role] = map[string][]string{}
		for _, table := range allTables {
			held := []string{}
			for _, priv := range tablePrivileges {
				if scanBool(ctx, t, admin, `SELECT has_table_privilege($1, $2, $3)`, role, "public."+table, priv) {
					held = append(held, priv)
				}
			}
			got[role][table] = held
		}
	}
	return got
}

// columnOnlyPrivileges lists, as "role table privilege", every privilege that a role holds on some
// column of a table and does not hold on the table. has_table_privilege, which the matrix is read
// with, cannot see a grant on a column: has_any_column_privilege can. In the role model there is none.
func columnOnlyPrivileges(ctx context.Context, t *testing.T, admin *pgxpool.Pool) []string {
	t.Helper()
	var found []string
	for _, role := range []string{dbtest.RoleApp, dbtest.RoleDBA} {
		for _, table := range allTables {
			for _, priv := range []string{"SELECT", "INSERT", "UPDATE", "REFERENCES"} {
				if scanBool(ctx, t, admin, `SELECT has_any_column_privilege($1, $2, $3) AND NOT has_table_privilege($1, $2, $3)`, role, "public."+table, priv) {
					found = append(found, role+" "+table+" "+priv)
				}
			}
		}
	}
	return found
}

func formatMatrix(m map[string]map[string][]string) string {
	var b strings.Builder
	for _, role := range []string{dbtest.RoleApp, dbtest.RoleDBA} {
		for _, table := range allTables {
			fmt.Fprintf(&b, "  %-12s %-18s %s\n", role, table, strings.Join(m[role][table], ", "))
		}
	}
	return b.String()
}

// The matrix as built: every role and table, against the role model. The matrix is
// printed under -v so a run can be read against docs/ROLES.md.
func TestRolePrivilegeMatrix(t *testing.T) {
	_, admin := migratedRoles(t)
	ctx := roleCtx(t)

	got, want := grantedMatrix(ctx, t, admin), expectedMatrix()
	t.Logf("privilege matrix as built:\n%s", formatMatrix(got))
	if !reflect.DeepEqual(got, want) {
		t.Errorf("privilege matrix =\n%s\nwant\n%s", formatMatrix(got), formatMatrix(want))
	}
	// has_table_privilege does not see a privilege on a column; nobody holds one.
	if got := columnOnlyPrivileges(ctx, t, admin); len(got) != 0 {
		t.Errorf("privileges held on columns only: %v, want none", got)
	}

	// No role but the owner may do anything to an identity sequence: the inserts the
	// application makes need no sequence privilege (TestAppRoleWritesEvidenceAndLedger).
	seqs := scanStrings(ctx, t, admin, `SELECT c.relname FROM pg_class c WHERE c.relkind = 'S' AND c.relnamespace = 'public'::regnamespace ORDER BY 1`)
	if len(seqs) == 0 {
		t.Fatal("found no identity sequences; the check below would be empty")
	}
	for _, role := range []string{dbtest.RoleApp, dbtest.RoleDBA} {
		for _, seq := range seqs {
			for _, priv := range []string{"USAGE", "SELECT", "UPDATE"} {
				if scanBool(ctx, t, admin, `SELECT has_sequence_privilege($1, $2, $3)`, role, "public."+seq, priv) {
					t.Errorf("%s holds %s on the sequence %s", role, priv, seq)
				}
			}
		}
	}

	// The schema and the database.
	for _, c := range []struct {
		role, priv string
		want       bool
	}{
		{dbtest.RoleApp, "USAGE", true},
		{dbtest.RoleApp, "CREATE", false},
		{dbtest.RoleDBA, "USAGE", true},
		{dbtest.RoleDBA, "CREATE", false},
		{dbtest.RoleMigrator, "USAGE", true},
		{dbtest.RoleMigrator, "CREATE", true},
	} {
		if got := scanBool(ctx, t, admin, `SELECT has_schema_privilege($1, 'public', $2)`, c.role, c.priv); got != c.want {
			t.Errorf("%s %s on the schema public = %v, want %v", c.role, c.priv, got, c.want)
		}
	}
	for _, c := range []struct {
		role, priv string
		want       bool
	}{
		{dbtest.RoleApp, "CONNECT", true},
		{dbtest.RoleApp, "CREATE", false},
		{dbtest.RoleApp, "TEMPORARY", false},
		{dbtest.RoleDBA, "CONNECT", true},
		{dbtest.RoleDBA, "CREATE", false},
		{dbtest.RoleDBA, "TEMPORARY", true},
		{dbtest.RoleMigrator, "CONNECT", true},
		{dbtest.RoleMigrator, "CREATE", true},
		{dbtest.RoleMigrator, "TEMPORARY", true},
	} {
		if got := scanBool(ctx, t, admin, `SELECT has_database_privilege($1, current_database(), $2)`, c.role, c.priv); got != c.want {
			t.Errorf("%s %s on the database = %v, want %v", c.role, c.priv, got, c.want)
		}
	}
	// PUBLIC holds nothing on the database: a role the script never heard of cannot connect, create
	// a temporary table or create a schema (roles_public_db_test.go has the role that tries).
	if scanBool(ctx, t, admin, `SELECT EXISTS (SELECT 1 FROM pg_database d, aclexplode(d.datacl) a WHERE d.datname = current_database() AND a.grantee = 0)`) {
		t.Error("PUBLIC holds a privilege on the database")
	}
	// PUBLIC may not create in the schema either: a role the script never heard of
	// gets no foothold there.
	if scanBool(ctx, t, admin, `SELECT EXISTS (SELECT 1 FROM pg_namespace n, aclexplode(n.nspacl) a WHERE n.nspname = 'public' AND a.grantee = 0 AND a.privilege_type = 'CREATE')`) {
		t.Error("PUBLIC holds CREATE on the schema public")
	}
}

// The migrator owns what it creates, and the other two roles own nothing: only an
// owner can drop a trigger or switch it off.
func TestRoleOwnership(t *testing.T) {
	_, admin := migratedRoles(t)
	ctx := roleCtx(t)

	owners := scanStrings(ctx, t, admin, `SELECT DISTINCT tableowner FROM pg_tables WHERE schemaname = 'public'`)
	if !slices.Equal(owners, []string{dbtest.RoleMigrator}) {
		t.Errorf("table owners = %v, want only %s", owners, dbtest.RoleMigrator)
	}
	tables := scanStrings(ctx, t, admin, `SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY 1`)
	if want := []string{"catalogue_generation", "catalogue_projection", "download_attempts", "raw_documents", "raw_redactions", "raw_search_pages", "schema_migrations", "sync_days", "sync_runs"}; !slices.Equal(tables, want) {
		t.Errorf("tables = %v, want %v", tables, want)
	}
	viewOwners := scanStrings(ctx, t, admin, `SELECT viewowner::text FROM pg_views WHERE schemaname = 'public' ORDER BY viewname`)
	if !slices.Equal(viewOwners, []string{dbtest.RoleMigrator}) {
		t.Errorf("view owners = %v, want the one view (raw_documents_live) owned by %s", viewOwners, dbtest.RoleMigrator)
	}
	funcOwners := scanStrings(ctx, t, admin, `SELECT DISTINCT pg_get_userbyid(p.proowner) FROM pg_proc p WHERE p.pronamespace = 'public'::regnamespace`)
	if !slices.Equal(funcOwners, []string{dbtest.RoleMigrator}) {
		t.Errorf("function owners = %v, want only %s", funcOwners, dbtest.RoleMigrator)
	}
	seqOwners := scanStrings(ctx, t, admin, `SELECT DISTINCT pg_get_userbyid(c.relowner) FROM pg_class c WHERE c.relkind = 'S' AND c.relnamespace = 'public'::regnamespace`)
	if !slices.Equal(seqOwners, []string{dbtest.RoleMigrator}) {
		t.Errorf("sequence owners = %v, want only %s", seqOwners, dbtest.RoleMigrator)
	}
	if got := scanStrings(ctx, t, admin, `SELECT pg_get_userbyid(datdba) FROM pg_database WHERE datname = current_database()`); !slices.Equal(got, []string{dbtest.RoleMigrator}) {
		t.Errorf("database owner = %v, want %s", got, dbtest.RoleMigrator)
	}

	for _, role := range []string{dbtest.RoleApp, dbtest.RoleDBA} {
		var owned int
		err := admin.QueryRow(ctx, `SELECT count(*) FROM pg_shdepend d WHERE d.refclassid = 'pg_authid'::regclass AND d.deptype = 'o' AND d.refobjid = (SELECT oid FROM pg_roles WHERE rolname = $1)`, role).Scan(&owned)
		if err != nil {
			t.Fatalf("count owned objects: %s", dbtest.Describe(err))
		}
		if owned != 0 {
			t.Errorf("%s owns %d objects, want none", role, owned)
		}
	}
}

// Attributes: none of the three roles can do anything beyond its privileges, and
// the two login roles cannot reach a schema by a search path of someone else's.
func TestRoleAttributes(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)

	type attrs struct {
		super, createDB, createRole, bypassRLS, replication, login bool
		member                                                     int
		config                                                     string
	}
	for _, c := range []struct {
		role  string
		login bool
		pin   bool
	}{
		{dbtest.RoleMigrator, true, true},
		{dbtest.RoleApp, true, true},
		{dbtest.RoleDBA, false, false},
	} {
		var a attrs
		err := admin.QueryRow(ctx, `
			SELECT r.rolsuper, r.rolcreatedb, r.rolcreaterole, r.rolbypassrls, r.rolreplication, r.rolcanlogin,
			       (SELECT count(*) FROM pg_auth_members m WHERE m.member = r.oid OR m.roleid = r.oid),
			       coalesce(array_to_string(r.rolconfig, ','), '')
			  FROM pg_roles r WHERE r.rolname = $1`, c.role).
			Scan(&a.super, &a.createDB, &a.createRole, &a.bypassRLS, &a.replication, &a.login, &a.member, &a.config)
		if err != nil {
			t.Fatalf("read %s: %s", c.role, dbtest.Describe(err))
		}
		if a.super || a.createDB || a.createRole || a.bypassRLS || a.replication {
			t.Errorf("%s has an attribute it must not: %+v", c.role, a)
		}
		if a.login != c.login {
			t.Errorf("%s LOGIN = %v, want %v", c.role, a.login, c.login)
		}
		if a.member != 0 {
			t.Errorf("%s is in %d role memberships, want none", c.role, a.member)
		}
		if got := strings.Contains(a.config, "search_path=public"); got != c.pin {
			t.Errorf("%s search_path pinned = %v, want %v (settings %q)", c.role, got, c.pin, a.config)
		}
	}

	// The pin holds at login: the app's own session resolves names in public only.
	app := dbtest.Pool(t, rs.AppDSN)
	if got := scanStrings(ctx, t, app, `SHOW search_path`); !slices.Equal(got, []string{"public"}) {
		t.Errorf("the app role's search_path = %v, want public", got)
	}
}

// A second run of the script, and a third, change nothing: not a role, not an owner,
// not one privilege or default. They leave a password alone, because the script has none.
func TestRolesScriptIsIdempotent(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)

	before := aclSnapshot(ctx, t, admin)
	if len(before) < 10 {
		t.Fatalf("the ACL snapshot has only %d rows; it is not looking at enough", len(before))
	}
	for run := 2; run <= 3; run++ {
		dbtest.RunRolesScript(t, rs)
		after := aclSnapshot(ctx, t, admin)
		if !slices.Equal(before, after) {
			t.Fatalf("run %d of scripts/roles.sql changed the ACLs:\nbefore:\n%s\nafter:\n%s", run, strings.Join(before, "\n"), strings.Join(after, "\n"))
		}
	}
	// The passwords the test set are still the roles' passwords.
	for _, dsn := range []string{rs.AppDSN, rs.MigratorDSN} {
		pool := dbtest.Pool(t, dsn)
		if err := pool.Ping(ctx); err != nil {
			t.Fatalf("a role cannot log in after the script ran again: %s", dbtest.Describe(err))
		}
	}
}

// aclSnapshot lists, in a stable order, every ACL, owner, default privilege and
// role setting the script can touch.
func aclSnapshot(ctx context.Context, t *testing.T, admin *pgxpool.Pool) []string {
	t.Helper()
	return scanStrings(ctx, t, admin, `
		SELECT 'rel ' || c.relname || ' ' || pg_get_userbyid(c.relowner) || ' ' || coalesce(c.relacl::text, '-')
		  FROM pg_class c WHERE c.relnamespace = 'public'::regnamespace
		UNION ALL
		SELECT 'fn ' || p.proname || ' ' || pg_get_userbyid(p.proowner) || ' ' || coalesce(p.proacl::text, '-')
		  FROM pg_proc p WHERE p.pronamespace = 'public'::regnamespace
		UNION ALL
		SELECT 'nsp ' || n.nspname || ' ' || pg_get_userbyid(n.nspowner) || ' ' || coalesce(n.nspacl::text, '-')
		  FROM pg_namespace n WHERE n.nspname = 'public'
		UNION ALL
		SELECT 'db ' || pg_get_userbyid(d.datdba) || ' ' || coalesce(d.datacl::text, '-')
		  FROM pg_database d WHERE d.datname = current_database()
		UNION ALL
		SELECT 'def ' || pg_get_userbyid(a.defaclrole) || ' ' || coalesce(a.defaclnamespace::regnamespace::text, '*') || ' ' || a.defaclobjtype::text || ' ' || a.defaclacl::text
		  FROM pg_default_acl a
		UNION ALL
		SELECT 'role ' || r.rolname || ' ' || r.rolsuper || r.rolcreatedb || r.rolcreaterole || r.rolbypassrls || r.rolreplication || r.rolcanlogin || ' ' || coalesce(array_to_string(r.rolconfig, ','), '-')
		  FROM pg_roles r WHERE r.rolname IN ('anbud_migrator', 'anbud_app', 'anbud_dba')
		ORDER BY 1`)
}

func TestCatalogueProjectionRolePrivileges(t *testing.T) {
	rs, admin := migratedRoles(t)
	app := dbtest.Pool(t, rs.AppDSN)
	ctx := roleCtx(t)
	for _, sql := range []string{`SELECT generation FROM catalogue_generation`, `SELECT raw_document_id FROM catalogue_projection`, `UPDATE catalogue_projection SET parser_version=parser_version WHERE false`} {
		if got := outcome(ctx, app, sql); got != "ok" {
			t.Fatalf("projection privilege missing: %s", got)
		}
	}
	// Exercise the real row lock/write path through the restricted runtime role.
	store := rawstore.New(app)
	day := time.Now().UTC().Truncate(24 * time.Hour)
	run, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: day, To: day})
	if err != nil {
		t.Fatal(err)
	}
	body := []byte("<notice/>")
	sum := sha256.Sum256(body)
	if _, err := store.StoreNotice(ctx, run, doffin.Notice{ID: "2026-100001", Body: body, SHA256: hex.EncodeToString(sum[:]), FetchedAt: day}); err != nil {
		t.Fatal(err)
	}
	pending, err := store.PendingCatalogue(ctx, 1)
	if err != nil || len(pending) != 1 {
		t.Fatalf("pending through app: %d %v", len(pending), err)
	}
	entry := rawstore.CatalogueProjection{Sequence: pending[0].Sequence, Payload: json.RawMessage(`{"notice":{"notice_id":"2026-100001"}}`)}
	if err := store.SaveCatalogueProjection(ctx, 1, []rawstore.CatalogueProjection{entry}); err != nil {
		t.Fatal(err)
	}
	if err := store.SetCataloguePublicationDates(ctx, 1, map[string]string{"2026-100001": "2026-10-01"}); err != nil {
		t.Fatal(err)
	}
	if _, err := admin.Exec(ctx, `SELECT anbud_redact_raw_document($1,'erasure_request','TEST-PROJECTION-ROLE')`, entry.Sequence); err != nil {
		t.Fatal(err)
	}
	if err := store.SaveCatalogueProjection(ctx, 1, []rawstore.CatalogueProjection{entry}); err != nil {
		t.Fatal(err)
	}
	held, err := store.PreparedCatalogue(ctx, 1, 0)
	if err != nil || len(held) != 0 {
		t.Fatalf("app resurrected erased source: %d %v", len(held), err)
	}

	for _, sql := range []string{`UPDATE catalogue_generation SET generation=0`, `DELETE FROM catalogue_projection`, `UPDATE raw_documents SET content_type='x' WHERE false`, `SELECT catalogue_source_changed()`} {
		if got := outcome(ctx, app, sql); got != "42501" {
			t.Fatalf("unwanted authority granted: %s", got)
		}
	}
}

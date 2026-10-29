package db_test

import (
	"fmt"
	"regexp"
	"slices"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// These tests read scripts/roles.sql and internal/db/privileges.sql as text and
// pin the properties a reviewer must be able to rely on without running them:
// no credential in the repository, nothing destructive, every role attribute
// switched off, and one atomic run. They are pure. The scripts' behaviour on a
// real server is in roles_db_test.go.

var lineComment = regexp.MustCompile(`--[^\n]*`)

// code is the script without its comments.
func code(script string) string { return lineComment.ReplaceAllString(script, "") }

func rolesScript(t *testing.T) string {
	t.Helper()
	return dbtest.ReadRolesScript(t)
}

// The DBA sets passwords out of band, so no password, hash or key is in the repository.
func TestRolesScriptCarriesNoCredential(t *testing.T) {
	t.Parallel()

	script := strings.ToLower(rolesScript(t))
	for _, word := range []string{"password", "encrypted", "md5", "scram", "secret", "valid until"} {
		if strings.Contains(script, word) {
			t.Errorf("scripts/roles.sql contains %q: credentials are set out of band, never here", word)
		}
	}
}

// Plain SQL and one psql variable. A backslash command, or any other variable,
// would not survive the tests' own expansion of the script, which pgx runs.
func TestRolesScriptUsesOnlyTheDBNameVariable(t *testing.T) {
	t.Parallel()

	script := rolesScript(t)
	for i, line := range strings.Split(script, "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), `\`) {
			t.Errorf("line %d is a psql backslash command: %q", i+1, line)
		}
	}
	ref := regexp.MustCompile(`(^|[^:]):(['"]?)([A-Za-z_][A-Za-z0-9_]*)['"]?`)
	for _, m := range ref.FindAllStringSubmatch(code(script), -1) {
		if m[3] != "dbname" {
			t.Errorf("scripts/roles.sql refers to the psql variable %q; only dbname is expanded", m[3])
		}
	}
	if !strings.Contains(script, `:"dbname"`) || !strings.Contains(script, `:'dbname'`) {
		t.Error(`scripts/roles.sql should use both :"dbname" (an identifier) and :'dbname' (a literal)`)
	}
}

// Every role attribute that could cross the boundary is switched off, by name, for
// every role the script creates, whether or not the role already existed.
func TestRolesScriptSwitchesEveryDangerousAttributeOff(t *testing.T) {
	t.Parallel()

	script := code(rolesScript(t))
	attr := regexp.MustCompile(`(?i)\b(NO)?(SUPERUSER|CREATEDB|CREATEROLE|BYPASSRLS|REPLICATION)\b`)
	for _, m := range attr.FindAllStringSubmatch(script, -1) {
		if m[1] == "" {
			t.Errorf("scripts/roles.sql grants %s", m[2])
		}
	}
	for _, role := range []string{"anbud_migrator", "anbud_app", "anbud_dba"} {
		alter := regexp.MustCompile(`(?is)ALTER ROLE ` + role + `\s+(?:NO)?LOGIN\s+NOSUPERUSER\s+NOCREATEDB\s+NOCREATEROLE\s+NOBYPASSRLS\s+NOREPLICATION\s*;`)
		if !alter.MatchString(script) {
			t.Errorf("scripts/roles.sql does not re-assert NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION for %s", role)
		}
	}
	if !regexp.MustCompile(`(?is)ALTER ROLE anbud_dba\s+NOLOGIN\b`).MatchString(script) {
		t.Error("anbud_dba is a group role and must be NOLOGIN")
	}
	for _, role := range []string{"anbud_migrator", "anbud_app"} {
		if !regexp.MustCompile(`(?is)ALTER ROLE ` + role + `\s+LOGIN\b`).MatchString(script) {
			t.Errorf("%s must be LOGIN", role)
		}
	}
}

// statementKinds returns the first word of every statement of the script outside its
// DO blocks, in order.
func statementKinds(script string) []string {
	blocks := regexp.MustCompile(`(?is)\$([A-Za-z_]*)\$.*?\$([A-Za-z_]*)\$`)
	var kinds []string
	for _, stmt := range strings.Split(blocks.ReplaceAllString(code(script), ""), ";") {
		if s := strings.TrimSpace(stmt); s != "" {
			kinds = append(kinds, strings.ToUpper(strings.Fields(s)[0]))
		}
	}
	return kinds
}

// The script only creates and grants. It drops nothing, never deletes or truncates a
// table, grants nothing to PUBLIC, and grants nothing onward. Revoking the
// destructive privileges from the DBA group is not the same as using them.
func TestRolesScriptNeverDropsOrOverGrants(t *testing.T) {
	t.Parallel()

	script := code(rolesScript(t))
	for _, banned := range []*regexp.Regexp{
		regexp.MustCompile(`(?i)\bDROP\b`),
		regexp.MustCompile(`(?i)\bDELETE\s+FROM\b`),
		regexp.MustCompile(`(?i)\bTRUNCATE\s+(TABLE|ONLY)\b`),
		regexp.MustCompile(`(?i)\bTO\s+PUBLIC\b`),
		regexp.MustCompile(`(?i)\bWITH\s+GRANT\s+OPTION\b`),
		regexp.MustCompile(`(?i)\bWITH\s+ADMIN\b`),
		regexp.MustCompile(`(?i)\bGRANT\s+ALL\b`),
		regexp.MustCompile(`(?i)\bGRANT\s+anbud_\w+\s+TO\b`),
	} {
		if loc := banned.FindString(script); loc != "" {
			t.Errorf("scripts/roles.sql contains %q", loc)
		}
	}
	allowed := map[string]bool{"BEGIN": true, "SELECT": true, "DO": true, "ALTER": true, "GRANT": true, "REVOKE": true, "COMMIT": true}
	for _, kind := range statementKinds(rolesScript(t)) {
		if !allowed[kind] {
			t.Errorf("scripts/roles.sql has a %s statement; it only sets up roles, ownership and privileges", kind)
		}
	}
}

// One transaction: a failure part way leaves nothing half applied, and a second
// run starts from either the old state or the new one.
func TestRolesScriptRunsAsOneTransaction(t *testing.T) {
	t.Parallel()

	kinds := statementKinds(rolesScript(t))
	if len(kinds) < 3 || kinds[0] != "BEGIN" || kinds[len(kinds)-1] != "COMMIT" {
		t.Errorf("scripts/roles.sql should begin with BEGIN and end with COMMIT; its statements start with %v", kinds)
	}
	for _, k := range kinds[1 : len(kinds)-1] {
		if k == "BEGIN" || k == "COMMIT" || k == "ROLLBACK" {
			t.Errorf("scripts/roles.sql ends or restarts the transaction inside itself with %s", k)
		}
	}
}

// The script verifies that the connection is the database -v dbname names, so a
// run against the wrong database changes nothing, and that its name starts with anbud:
// the roles have fixed names and belong to the cluster, so no other database may be prepared.
func TestRolesScriptChecksItsTarget(t *testing.T) {
	t.Parallel()

	script := code(rolesScript(t))
	for _, want := range []string{"current_database()", "RAISE EXCEPTION"} {
		if !strings.Contains(script, want) {
			t.Errorf("scripts/roles.sql does not contain %q: it must check that it runs on the database it was told to prepare", want)
		}
	}
	first := strings.Index(script, "RAISE EXCEPTION")
	for _, change := range []string{"CREATE ROLE", "ALTER ROLE", "ALTER DATABASE", "GRANT", "REVOKE", "ALTER DEFAULT PRIVILEGES"} {
		if at := strings.Index(script, change); at >= 0 && at < first {
			t.Errorf("scripts/roles.sql changes something (%s) before it has checked its target", change)
		}
	}
	prefix := regexp.MustCompile(`(?is)left\(\s*current_database\(\)\s*,\s*5\s*\)\s+IS\s+DISTINCT\s+FROM\s+'anbud'`)
	at := prefix.FindStringIndex(script)
	if at == nil {
		t.Fatal("scripts/roles.sql does not check that the database name starts with anbud")
	}
	if raise := strings.Index(script[at[1]:], "RAISE EXCEPTION"); raise < 0 {
		t.Error("the check of the database name does not raise")
	}
	for _, change := range []string{"CREATE ROLE", "ALTER ROLE", "ALTER DATABASE", "GRANT", "REVOKE", "ALTER DEFAULT PRIVILEGES"} {
		if i := strings.Index(script, change); i >= 0 && i < at[0] {
			t.Errorf("scripts/roles.sql changes something (%s) before it has checked the name of the database", change)
		}
	}
}

// The script takes PUBLIC's CONNECT and TEMPORARY on the database away, keeps the explicit CONNECT
// of the three roles, and gives TEMPORARY to the DBA group and to nobody else: the application
// has none. Read as text, so the statements are pinned before any server runs them.
func TestRolesScriptHandsOutDatabasePrivilegesByName(t *testing.T) {
	t.Parallel()

	script := code(rolesScript(t))
	onDatabase := regexp.MustCompile(`(?is)\b(GRANT|REVOKE)\s+([A-Z, ]+?)\s+ON\s+DATABASE\s+:"dbname"\s+(TO|FROM)\s+([a-z_, ]+?)\s*;`)
	type stmt struct{ verb, privs, who string }
	var got []stmt
	for _, m := range onDatabase.FindAllStringSubmatch(script, -1) {
		privs := strings.Join(strings.Fields(strings.ToUpper(m[2])), " ")
		got = append(got, stmt{strings.ToUpper(m[1]), privs, strings.Join(strings.Fields(m[4]), " ")})
	}
	has := func(verb, privs, who string) bool { return slices.Contains(got, stmt{verb, privs, who}) }
	for _, want := range []stmt{
		{"REVOKE", "CONNECT, TEMPORARY, CREATE", "PUBLIC"},
		{"REVOKE", "CREATE, TEMPORARY", "anbud_app"},
		{"GRANT", "CONNECT", "anbud_migrator, anbud_app, anbud_dba"},
		{"GRANT", "TEMPORARY", "anbud_dba"},
	} {
		if !has(want.verb, want.privs, want.who) {
			t.Errorf("scripts/roles.sql lacks %s %s ON DATABASE :\"dbname\" %s; its statements on the database are %v", want.verb, want.privs, want.who, got)
		}
	}
	for _, g := range got {
		if g.verb == "GRANT" && strings.Contains(g.privs, "TEMPORARY") && g.who != "anbud_dba" {
			t.Errorf("scripts/roles.sql grants TEMPORARY to %s; only anbud_dba may hold it", g.who)
		}
		if g.verb == "GRANT" && strings.Contains(g.privs, "CREATE") {
			t.Errorf("scripts/roles.sql grants CREATE on the database to %s; the owner has it", g.who)
		}
	}
}

// The script takes back every membership that any of the three roles holds, with a DO block
// that reads the membership catalog and revokes each row, and never grants one.
func TestRolesScriptRevokesTheMembershipsOfTheThreeRoles(t *testing.T) {
	t.Parallel()

	script := code(rolesScript(t))
	blocks, _ := splitDoBlocks(script)
	var mine []string
	for _, b := range blocks {
		if strings.Contains(b, "pg_auth_members") {
			mine = append(mine, strings.Join(strings.Fields(b), " "))
		}
	}
	if len(mine) != 1 {
		t.Fatalf("scripts/roles.sql has %d blocks that read pg_auth_members, want exactly one", len(mine))
	}
	block := mine[0]
	for _, want := range []string{
		"WHERE pg_get_userbyid(am.member) IN ('anbud_migrator', 'anbud_app', 'anbud_dba')",
		"REVOKE %I FROM %I GRANTED BY %I CASCADE", // PostgreSQL 16 and later: each grant by its own grantor
		"REVOKE %I FROM %I CASCADE",               // before 16
		"server_version_num",
	} {
		if !strings.Contains(block, want) {
			t.Errorf("the membership block lacks %q", want)
		}
	}
	// It selects on the member only: the DBAs that are members of anbud_dba are not touched.
	if strings.Contains(block, "am.roleid IN") || strings.Contains(block, "pg_get_userbyid(am.roleid) IN") {
		t.Error("the membership block selects on the group as well; the human DBAs' memberships of anbud_dba must stay")
	}
}

// The comments of the script say what it does. The one on the search path no longer claims the
// roles resolve names in public and nowhere else: pg_catalog and pg_temp are always searched.
func TestRolesScriptCommentsAreAccurate(t *testing.T) {
	t.Parallel()

	script := rolesScript(t)
	if strings.Contains(script, "public and nowhere else") {
		t.Error(`the comment on the search path still says the roles resolve names "in public and nowhere else"`)
	}
	pin := regexp.MustCompile(`(?s)-- The two login roles name the schema public.*?\nALTER ROLE anbud_migrator SET search_path`).FindString(script)
	for _, want := range []string{"pg_catalog", "pg_temp", "TEMPORARY"} {
		if !strings.Contains(pin, want) {
			t.Errorf("the comment on the search path does not mention %s", want)
		}
	}
	header := strings.Join(strings.Fields(regexp.MustCompile(`(?m)^--`).ReplaceAllString(script[:strings.Index(script, "BEGIN;")], "")), " ")
	for _, want := range []string{"EXECUTE on the redaction function", "members of no role", "start with anbud", "TEMPORARY"} {
		if !strings.Contains(header, want) {
			t.Errorf("the header of scripts/roles.sql does not say %q", want)
		}
	}
}

// manifestEvidenceAndLedger are the tables the application may read and insert into.
var manifestEvidenceAndLedger = []string{"raw_search_pages", "raw_documents", "download_attempts", "sync_runs", "sync_days"}

// manifestProblems reads the statements of a privileges script and reports every GRANT that steps
// outside the role model. Every GRANT must have exactly one shape, GRANT <privileges> ON <table
// names> TO anbud_app, with nothing after the role (no WITH GRANT OPTION), no column list on the
// privileges, no ALL TABLES IN SCHEMA and no qualified name, so that nothing can be hidden in a
// clause the checker does not read. Within that shape only three grants exist: SELECT and INSERT
// on the evidence tables and the ledger, SELECT alone on the tombstones and the view of
// migration 0006, and UPDATE on the two ledger tables. A REVOKE is never a problem.
func manifestProblems(script string) []string {
	var problems []string
	grantWord := regexp.MustCompile(`(?i)\bGRANT\b`)
	shape := regexp.MustCompile(`(?is)^GRANT\s+([A-Z]+(?:\s*,\s*[A-Z]+)*)\s+ON\s+([a-z_][a-z0-9_]*(?:\s*,\s*[a-z_][a-z0-9_]*)*)\s+TO\s+([a-z_][a-z0-9_]*)$`)
	for _, stmt := range strings.Split(code(script), ";") {
		at := grantWord.FindStringIndex(stmt)
		if at == nil {
			continue
		}
		g := strings.Join(strings.Fields(stmt[at[0]:]), " ")
		if strings.HasPrefix(strings.ToUpper(g), "GRANT OPTION") {
			continue // the tail of REVOKE GRANT OPTION FOR, which takes and does not give
		}
		m := shape.FindStringSubmatch(g)
		if m == nil {
			problems = append(problems, fmt.Sprintf("%q is not of the form GRANT <privileges> ON <table names> TO anbud_app", g))
			continue
		}
		privs := strings.Join(strings.Fields(strings.ToUpper(strings.ReplaceAll(m[1], ",", " , "))), " ")
		privs = strings.ReplaceAll(privs, " ,", ",")
		var tables []string
		for _, table := range strings.Split(m[2], ",") {
			tables = append(tables, strings.ToLower(strings.TrimSpace(table)))
		}
		slices.Sort(tables)
		if !strings.EqualFold(m[3], "anbud_app") {
			problems = append(problems, fmt.Sprintf("%q grants to %s; the manifest manages anbud_app only", g, m[3]))
		}
		switch privs {
		case "SELECT, INSERT":
			for _, table := range tables {
				if !slices.Contains(manifestEvidenceAndLedger, table) {
					problems = append(problems, fmt.Sprintf("%q grants SELECT and INSERT on %s, which is not an evidence or ledger table", g, table))
				}
			}
		case "SELECT":
			if !slices.Equal(tables, []string{"raw_documents_live", "raw_redactions"}) && !slices.Equal(tables, []string{"catalogue_generation"}) {
				problems = append(problems, fmt.Sprintf("%q: SELECT alone is limited to the tombstones and the view of migration 0006", g))
			}
		case "SELECT, INSERT, UPDATE":
			if !slices.Equal(tables, []string{"catalogue_projection"}) {
				problems = append(problems, fmt.Sprintf("%q: mutable projection privileges must name only catalogue_projection", g))
			}
		case "UPDATE":
			if !slices.Equal(tables, []string{"sync_days", "sync_runs"}) {
				problems = append(problems, fmt.Sprintf("%q: UPDATE is limited to the two ledger tables, sync_runs and sync_days", g))
			}
		default:
			problems = append(problems, fmt.Sprintf("%q grants %s, which the role model does not give the application", g, privs))
		}
	}
	return problems
}

// The privileges the runner re-asserts after every migration run. The manifest
// grants only what the role model allows: SELECT and INSERT on the known tables,
// UPDATE on the two ledger tables and nothing destructive.
func TestPrivilegesManifestStaysInsideTheRoleModel(t *testing.T) {
	t.Parallel()

	script := code(db.PrivilegesSQL)
	if strings.TrimSpace(script) == "" {
		t.Fatal("the embedded privileges script is empty")
	}
	if n := len(regexp.MustCompile(`(?i)\bGRANT\s+(?:SELECT|UPDATE)\b`).FindAllString(script, -1)); n < 3 {
		t.Fatalf("the privileges script has %d table grants, want at least the three of the role model; the checker would read too little", n)
	}
	for _, problem := range manifestProblems(db.PrivilegesSQL) {
		t.Error(problem)
	}
}

// The checker is tested on its own, because a checker that cannot fail proves nothing. It gets
// the grants a hurried change might add, and each must be flagged, among them four that an
// earlier version of it let through; the grants of the real manifest, alone, must pass.
func TestPrivilegesManifestCheckerFlagsWhatItMustFlag(t *testing.T) {
	t.Parallel()

	for _, ok := range []string{
		`GRANT SELECT, INSERT ON raw_search_pages, raw_documents, download_attempts, sync_runs, sync_days TO anbud_app;`,
		`GRANT UPDATE ON sync_runs, sync_days TO anbud_app;`,
		`GRANT SELECT ON raw_redactions, raw_documents_live TO anbud_app;`,
		`GRANT select, insert on sync_days to anbud_app;`,
		`REVOKE GRANT OPTION FOR SELECT ON raw_documents FROM anbud_app;`,
		`REVOKE UPDATE, DELETE ON TABLE raw_documents FROM anbud_app;`,
	} {
		if problems := manifestProblems(ok); len(problems) != 0 {
			t.Errorf("the checker flags %q, which the role model allows: %v", ok, problems)
		}
	}
	for _, bad := range []string{
		// the four an earlier version of the checker did not flag
		`GRANT SELECT ON raw_documents TO anbud_app WITH GRANT OPTION;`,
		`GRANT UPDATE ON sync_runs, sync_days TO anbud_app WITH GRANT OPTION;`,
		`GRANT UPDATE (content_type) ON raw_documents TO anbud_app;`,
		`GRANT SELECT, INSERT ON ALL TABLES IN SCHEMA public TO anbud_app;`,
		// the same through the end of the statement, with no semicolon to end the clause
		`GRANT SELECT, INSERT ON raw_documents TO anbud_app WITH GRANT OPTION`,
		// other ways to give too much
		`GRANT SELECT ON raw_documents TO anbud_app;`,
		`GRANT SELECT, INSERT, UPDATE ON sync_runs TO anbud_app;`,
		`GRANT UPDATE ON raw_documents TO anbud_app;`,
		`GRANT UPDATE ON sync_runs TO anbud_app;`,
		`GRANT DELETE ON sync_runs TO anbud_app;`,
		`GRANT TRUNCATE ON raw_documents TO anbud_app;`,
		`GRANT TRIGGER ON raw_documents TO anbud_app;`,
		`GRANT REFERENCES ON raw_documents TO anbud_app;`,
		`GRANT ALL ON raw_documents TO anbud_app;`,
		`GRANT ALL PRIVILEGES ON raw_documents TO anbud_app;`,
		`GRANT SELECT, INSERT ON raw_documents TO PUBLIC;`,
		`GRANT SELECT, INSERT ON raw_documents TO anbud_dba;`,
		`GRANT SELECT, INSERT ON raw_documents TO anbud_app, anbud_dba;`,
		`GRANT SELECT, INSERT ON schema_migrations TO anbud_app;`,
		`GRANT SELECT, INSERT ON public.raw_documents TO anbud_app;`,
		`GRANT SELECT (id) ON raw_documents TO anbud_app;`,
		`GRANT INSERT (body) ON raw_documents TO anbud_app;`,
		`GRANT USAGE ON SEQUENCE raw_documents_id_seq TO anbud_app;`,
		`GRANT anbud_migrator TO anbud_app;`,
		`GRANT EXECUTE ON FUNCTION anbud_redact_raw_document(bigint, text, text) TO anbud_app;`,
		`GRANT SELECT ON raw_redactions TO anbud_app;`,
		`GRANT SELECT ON raw_redactions, raw_documents_live, raw_documents TO anbud_app;`,
	} {
		if problems := manifestProblems(bad); len(problems) == 0 {
			t.Errorf("the checker does not flag %q", bad)
		}
	}
	// A bad statement among good ones is still found.
	mixed := `GRANT SELECT, INSERT ON sync_days TO anbud_app; GRANT UPDATE (content_type) ON raw_documents TO anbud_app; GRANT UPDATE ON sync_runs, sync_days TO anbud_app;`
	if problems := manifestProblems(mixed); len(problems) != 1 {
		t.Errorf("the checker found %d problems in a script with one bad grant among two good ones: %v", len(problems), problems)
	}
	// And one inside a DO block, which is where the real manifest keeps its grants.
	block := "DO $x$ BEGIN IF true THEN GRANT SELECT ON raw_documents TO anbud_app WITH GRANT OPTION; END IF; END $x$;"
	if problems := manifestProblems(block); len(problems) == 0 {
		t.Error("the checker does not flag a grant inside a DO block")
	}
}

// Every table the manifest names exists: a table renamed by a later migration must
// fail here, in a pure test, and not at the first run against a database.
func TestPrivilegesManifestNamesTablesThatExist(t *testing.T) {
	t.Parallel()

	created := map[string]bool{"schema_migrations": true} // the runner creates it
	create := regexp.MustCompile(`(?im)^\s*CREATE (?:TABLE|VIEW) (?:IF NOT EXISTS )?([a-z_]+)`)
	for _, body := range db.EmbeddedMigrationBodies(t) {
		for _, m := range create.FindAllStringSubmatch(code(body), -1) {
			created[m[1]] = true
		}
	}
	named := regexp.MustCompile(`(?is)\b(?:GRANT\s+[A-Z, ]+?|REVOKE\s+[A-Z, ]+?)\s+ON\s+(?:TABLE\s+)?([a-z_, ]+?)\s+(?:TO|FROM)\s+anbud_app\b`)
	seen := 0
	for _, m := range named.FindAllStringSubmatch(code(db.PrivilegesSQL), -1) {
		for _, table := range strings.Split(m[1], ",") {
			table = strings.TrimSpace(table)
			if table == "ALL" || strings.HasPrefix(table, "ALL ") {
				continue
			}
			seen++
			if !created[table] {
				t.Errorf("the privileges script names %q, which no migration creates", table)
			}
		}
	}
	if seen == 0 {
		t.Error("found no table named in the privileges script; the pattern in this test is out of date")
	}
	for _, evidence := range []string{"raw_search_pages", "raw_documents", "download_attempts", "sync_runs", "sync_days"} {
		if !created[evidence] {
			t.Errorf("the migrations no longer create %s", evidence)
		}
	}
}

package db_test

import (
	"regexp"
	"slices"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db"
)

// Pure tests of migration 0006 (the redaction mechanism, open owner decision O1) and of
// the guarded block that scripts/roles.sql carries for it. They read the two files as
// text, so they run in make test and fail before any database test does. What the SQL
// does on a server is in redaction_function_db_test.go, redaction_guard_db_test.go and
// redaction_privileges_db_test.go.

const redactionFunctionName = "anbud_redact_raw_document"

var (
	// doBlock matches a DO block with a tagged dollar quote. The blocks in these two
	// files hold no dollar quote of their own.
	doBlock = regexp.MustCompile(`(?is)\bDO\s+\$([A-Za-z_]*)\$(.*?)\$([A-Za-z_]*)\$\s*;`)

	// dollarQuoted matches any tagged dollar-quoted region, a function body included.
	dollarQuoted = regexp.MustCompile(`(?is)\$([A-Za-z_]*)\$.*?\$([A-Za-z_]*)\$`)

	// roleName matches a role of the role model.
	roleName = regexp.MustCompile(`\banbud_(migrator|app|dba)\b`)
)

// redactionSignature matches the function's signature in either case: migration 0006
// writes the types in capitals and scripts/roles.sql, which names them for
// to_regprocedure, in lower case.
const redactionSignature = `anbud_redact_raw_document\(\s*(?:BIGINT|bigint)\s*,\s*(?:TEXT|text)\s*,\s*(?:TEXT|text)\s*\)`

// splitDoBlocks returns the bodies of the DO blocks of sql and sql without them.
func splitDoBlocks(sql string) (blocks []string, rest string) {
	for _, m := range doBlock.FindAllStringSubmatch(sql, -1) {
		blocks = append(blocks, m[2])
	}
	return blocks, doBlock.ReplaceAllString(sql, "")
}

// sqlStatements splits sql, without its comments and its dollar-quoted regions, into
// its statements, with white space collapsed.
func sqlStatements(sql string) []string {
	var out []string
	for _, stmt := range strings.Split(dollarQuoted.ReplaceAllString(code(sql), ""), ";") {
		if s := strings.Join(strings.Fields(stmt), " "); s != "" {
			out = append(out, s)
		}
	}
	return out
}

// functionParts returns what lies between CREATE FUNCTION name( and the dollar quote that
// opens the body (the header), and the body, with comments removed.
func functionParts(t *testing.T, sql, name string) (header, body string) {
	t.Helper()
	text := code(sql)
	start := strings.Index(text, "CREATE FUNCTION "+name+"(")
	if start < 0 {
		t.Fatalf("the SQL does not create the function %s", name)
	}
	rest := text[start:]
	open := regexp.MustCompile(`\$[A-Za-z_]*\$`).FindStringIndex(rest)
	if open == nil {
		t.Fatalf("%s has no dollar-quoted body", name)
	}
	tag := rest[open[0]:open[1]]
	end := strings.Index(rest[open[1]:], tag)
	if end < 0 {
		t.Fatalf("%s: the dollar quote %s is never closed", name, tag)
	}
	return rest[:open[0]], rest[open[1] : open[1]+end]
}

func redactionMigration(t *testing.T) string {
	t.Helper()
	name, body := db.EmbeddedMigration(t, 6)
	if name != "0006_redaction" {
		t.Fatalf("migration 6 is %q, want 0006_redaction", name)
	}
	return body
}

// The migration runs inside the runner's transaction, as 0002 to 0005 do.
func TestMigration0006NeverEndsTheTransaction(t *testing.T) {
	t.Parallel()

	kinds := statementKinds(redactionMigration(t))
	if len(kinds) < 10 {
		t.Fatalf("the migration has only %d statements outside its function bodies; the check below would prove nothing", len(kinds))
	}
	for _, kind := range kinds {
		switch kind {
		case "BEGIN", "COMMIT", "ROLLBACK", "SAVEPOINT", "RELEASE", "START", "END":
			t.Errorf("migration 0006 has a %s statement: the runner owns the transaction", kind)
		}
	}
}

// Exactly the two column checks that forbid an erased body are replaced, under their own
// names (the ones PostgreSQL gave them in 0002, read from pg_constraint: the hash check
// names two columns, so it is raw_documents_check), and each holds only while the row is
// live. The hash check keeps its format test for every row.
func TestMigration0006RelaxesTwoChecksAndOnlyWhileTheBodyIsHeld(t *testing.T) {
	t.Parallel()

	statements := sqlStatements(redactionMigration(t))
	var dropped []string
	added := map[string]string{}
	notValid := map[string]bool{}
	drop := regexp.MustCompile(`(?i)^ALTER TABLE raw_documents DROP CONSTRAINT (\w+)$`)
	add := regexp.MustCompile(`(?i)^ALTER TABLE raw_documents ADD CONSTRAINT (\w+) CHECK \((.*)\)( NOT VALID)?$`)
	for _, stmt := range statements {
		if m := drop.FindStringSubmatch(stmt); m != nil {
			dropped = append(dropped, m[1])
		}
		if m := add.FindStringSubmatch(stmt); m != nil {
			added[m[1]] = m[2]
			notValid[m[1]] = m[3] != ""
		}
	}
	// Each check is added NOT VALID: validating would hash every stored body under an
	// exclusive lock, for minutes on a full corpus, and every row that exists already
	// satisfies the stronger checks of 0002, so the scan could not find anything. The
	// upgrade test validates them afterwards to show it.
	for name := range added {
		if !notValid[name] {
			t.Errorf("%s is added validated: that scans every body under an exclusive lock, and every existing row already satisfies it", name)
		}
	}
	slices.Sort(dropped)
	if want := []string{"raw_documents_body_check", "raw_documents_check"}; !slices.Equal(dropped, want) {
		t.Errorf("constraints dropped from raw_documents = %v, want exactly %v", dropped, want)
	}
	for _, name := range dropped {
		def, ok := added[name]
		if !ok {
			t.Errorf("%s is dropped and not added again under its own name", name)
			continue
		}
		if !strings.Contains(def, "redacted_at IS NOT NULL OR") {
			t.Errorf("%s does not let a redacted row through: %q", name, def)
		}
	}
	hash := added["raw_documents_check"]
	format := strings.Index(hash, `body_sha256 ~ '^[0-9a-f]{64}$'`)
	live := strings.Index(hash, "redacted_at IS NOT NULL OR")
	if format < 0 || live < 0 || format > live {
		t.Errorf("the hash check must test the format of every row and relax only the equality with the body: %q", hash)
	}
	if def := added["raw_documents_redacted_is_empty"]; !strings.Contains(def, "redacted_at IS NULL OR octet_length(body) = 0") {
		t.Errorf("a redacted row must hold an empty body by constraint: %q", def)
	}
}

// A role of the role model is named only inside a DO block that first looks the role up, so
// the migration works on a database where no anbud_ role exists, whatever order the
// roles script and the migrations ran in.
func TestMigration0006NamesNoRoleOutsideAGuardedBlock(t *testing.T) {
	t.Parallel()

	blocks, rest := splitDoBlocks(code(redactionMigration(t)))
	if len(blocks) == 0 {
		t.Fatal("the migration has no DO block, so it grants and revokes nothing")
	}
	if m := roleName.FindString(rest); m != "" {
		t.Errorf("migration 0006 names the role %s outside a DO block that checks the role exists", m)
	}
	for _, block := range blocks {
		for _, role := range roleName.FindAllString(block, -1) {
			guard := regexp.MustCompile(`(?is)IF\s+EXISTS\s*\(\s*SELECT\s+1\s+FROM\s+(?:pg_catalog\.)?pg_roles\s+WHERE\s+rolname\s*=\s*'` + role + `'\s*\)\s*THEN`)
			if !guard.MatchString(block) {
				t.Errorf("a DO block of migration 0006 names %s without testing that the role exists", role)
			}
		}
	}
}

// The migration states the function's privileges itself and does not lean on the default
// privileges of scripts/roles.sql, which may not have run yet: PUBLIC is revoked in the
// open, anbud_dba is the only grantee, anbud_app is refused INSERT on the tombstones.
func TestMigration0006StatesItsPrivilegesItself(t *testing.T) {
	t.Parallel()

	body := redactionMigration(t)
	blocks, rest := splitDoBlocks(code(body))

	revokePublic := regexp.MustCompile(`(?is)\bREVOKE\s+ALL\s+ON\s+FUNCTION\s+` + redactionSignature + `\s+FROM\s+PUBLIC\s*;`)
	if !revokePublic.MatchString(rest) {
		t.Error("the migration does not revoke the function from PUBLIC outside a DO block")
	}
	for _, banned := range []*regexp.Regexp{
		regexp.MustCompile(`(?i)\bTO\s+PUBLIC\b`),
		regexp.MustCompile(`(?i)\bGRANT\s+ALL\b`),
		regexp.MustCompile(`(?i)\bWITH\s+GRANT\s+OPTION\b`),
	} {
		if loc := banned.FindString(code(body)); loc != "" {
			t.Errorf("migration 0006 contains %q", loc)
		}
	}

	var grants, revokes []string
	for _, stmt := range sqlStatementsOfBlocks(blocks) {
		upper := strings.ToUpper(stmt)
		switch {
		case strings.HasPrefix(upper, "GRANT "):
			grants = append(grants, stmt)
		case strings.HasPrefix(upper, "REVOKE "):
			revokes = append(revokes, stmt)
		}
	}
	onlyDBA := regexp.MustCompile(`(?i)^GRANT EXECUTE ON FUNCTION ` + redactionSignature + ` TO anbud_dba$`)
	if len(grants) != 1 || !onlyDBA.MatchString(grants[0]) {
		t.Errorf("the migration's grants = %q, want exactly EXECUTE on the function to anbud_dba", grants)
	}
	wantRevokes := []string{"REVOKE INSERT ON raw_documents_live FROM anbud_app", "REVOKE INSERT ON raw_redactions FROM anbud_app"}
	slices.Sort(revokes)
	if !slices.Equal(revokes, wantRevokes) {
		t.Errorf("the migration's guarded revokes = %q, want %q", revokes, wantRevokes)
	}
	if created, revoked := strings.Index(code(body), "CREATE TABLE raw_redactions"), strings.Index(code(body), "REVOKE INSERT ON raw_redactions"); created < 0 || revoked < created {
		t.Error("INSERT on the tombstones is not revoked from anbud_app after the table is created")
	}
}

// sqlStatementsOfBlocks splits the bodies of DO blocks into statements, and drops the
// plpgsql scaffolding (IF, THEN, END IF, BEGIN, END) that is not a statement of its own.
func sqlStatementsOfBlocks(blocks []string) []string {
	scaffold := regexp.MustCompile(`(?is)\b(?:END\s+IF|IF\b.*?\bTHEN|BEGIN|END|DECLARE)\b`)
	var out []string
	for _, block := range blocks {
		for _, stmt := range strings.Split(scaffold.ReplaceAllString(block, ";"), ";") {
			if s := strings.Join(strings.Fields(stmt), " "); s != "" {
				out = append(out, s)
			}
		}
	}
	return out
}

// Every function the migration adds pins its search path, with pg_temp last: a role that
// can create a temporary table must not be able to stand in for a table of the function.
func TestMigration0006FunctionsPinTheSearchPathWithTempLast(t *testing.T) {
	t.Parallel()

	body := redactionMigration(t)
	for _, name := range []string{redactionFunctionName, "raw_documents_change_guard", "raw_documents_insert_guard"} {
		header, _ := functionParts(t, body, name)
		if !strings.Contains(header, "SET search_path = pg_catalog, public, pg_temp") {
			t.Errorf("%s does not pin SET search_path = pg_catalog, public, pg_temp", name)
		}
		if n := strings.Count(header, "search_path"); n != 1 {
			t.Errorf("%s sets search_path %d times, want once", name, n)
		}
	}
	header, _ := functionParts(t, body, redactionFunctionName)
	if !strings.Contains(header, "SECURITY DEFINER") {
		t.Error("the redaction function is not SECURITY DEFINER: the DBA has no privilege to erase and must not get one")
	}
}

// Inside the redaction function every table is named with its schema and every built-in
// with pg_catalog, so nothing in a temporary schema or on a search path can be picked
// instead. The search path is a second line of defence, not the first.
func TestMigration0006FunctionNamesEveryObjectBySchema(t *testing.T) {
	t.Parallel()

	_, fn := functionParts(t, redactionMigration(t), redactionFunctionName)

	relation := regexp.MustCompile(`\b(raw_documents_live|raw_documents|raw_redactions)\b`)
	seen := 0
	for _, at := range relation.FindAllStringIndex(fn, -1) {
		seen++
		if !strings.HasSuffix(fn[:at[0]], "public.") {
			t.Errorf("the function names %q without public.", fn[at[0]:at[1]])
		}
	}
	if seen < 3 {
		t.Fatalf("the function names %d tables; the check above looks at too little", seen)
	}
	builtin := regexp.MustCompile(`(^|[^.\w])(set_config|txid_current|octet_length|transaction_timestamp|current_setting)\s*\(`)
	if m := builtin.FindStringSubmatch(fn); m != nil {
		t.Errorf("the function calls %s without pg_catalog.", m[2])
	}
	for _, want := range []string{"pg_catalog.set_config(", "pg_catalog.txid_current()", "pg_catalog.octet_length(", "pg_catalog.transaction_timestamp()"} {
		if !strings.Contains(fn, want) {
			t.Errorf("the function does not call %s: the check above would pass for the wrong reason", want)
		}
	}
}

// The function writes one log line, with ids and the reason, and nothing from the body.
// It also never carries the caller's request reference, which may be a name by mistake.
func TestMigration0006FunctionLogsIdsOnly(t *testing.T) {
	t.Parallel()

	_, fn := functionParts(t, redactionMigration(t), redactionFunctionName)
	logs := regexp.MustCompile(`(?is)RAISE\s+LOG\b(.*?);`).FindAllStringSubmatch(fn, -1)
	if len(logs) != 1 {
		t.Fatalf("the function has %d RAISE LOG statements, want one", len(logs))
	}
	line := logs[0][1]
	for _, banned := range []string{"body", "p_request", "request_ref"} {
		if strings.Contains(strings.ToLower(line), banned) {
			t.Errorf("the log line mentions %q: %s", banned, strings.Join(strings.Fields(line), " "))
		}
	}
	if !strings.Contains(line, "p_id") {
		t.Error("the log line does not carry the document id")
	}
}

// The reason list and the request shape are written twice, in the tombstone's checks and
// in the function that validates before it writes, and they must agree: a function that
// accepted what the table refuses would fail late and echo the row.
func TestMigration0006TheFunctionAndTheTableAgreeOnWhatTheyAccept(t *testing.T) {
	t.Parallel()

	body := redactionMigration(t)
	_, fn := functionParts(t, body, redactionFunctionName)
	literals := regexp.MustCompile(`'([a-z_]+)'`)

	tableReasons := regexp.MustCompile(`(?is)reason\s+TEXT\s+NOT NULL\s+CHECK\s*\(\s*reason\s+IN\s*\((.*?)\)\s*\)`).FindStringSubmatch(code(body))
	fnReasons := regexp.MustCompile(`(?is)p_reason\s+NOT\s+IN\s*\((.*?)\)`).FindStringSubmatch(fn)
	if tableReasons == nil || fnReasons == nil {
		t.Fatalf("could not find the reason lists (table %v, function %v)", tableReasons != nil, fnReasons != nil)
	}
	list := func(text string) []string {
		var out []string
		for _, m := range literals.FindAllStringSubmatch(text, -1) {
			out = append(out, m[1])
		}
		slices.Sort(out)
		return out
	}
	want := []string{"authority_order", "controller_decision", "erasure_request", "objection", "source_withdrawn"}
	if got := list(tableReasons[1]); !slices.Equal(got, want) {
		t.Errorf("the tombstone's reasons = %v, want %v", got, want)
	}
	if got := list(fnReasons[1]); !slices.Equal(got, want) {
		t.Errorf("the function's reasons = %v, want %v", got, want)
	}

	tableShape := regexp.MustCompile(`(?is)request_ref\s+TEXT\s+NOT NULL\s+CHECK\s*\(\s*request_ref\s+~\s+'([^']+)'`).FindStringSubmatch(code(body))
	fnShape := regexp.MustCompile(`(?is)p_request\s+!~\s+'([^']+)'`).FindStringSubmatch(fn)
	if tableShape == nil || fnShape == nil {
		t.Fatalf("could not find the request shapes (table %v, function %v)", tableShape != nil, fnShape != nil)
	}
	if tableShape[1] != fnShape[1] || tableShape[1] != `^[A-Za-z0-9._:/-]{1,120}$` {
		t.Errorf("request shapes differ: table %q, function %q, want ^[A-Za-z0-9._:/-]{1,120}$ in both", tableShape[1], fnShape[1])
	}
}

// The guarded block of scripts/roles.sql: the function is named only inside one DO block
// that first checks it exists (migrations may run before or after the script), the only
// EXECUTE grant is to anbud_dba, and the revokes name nobody else. The block comes after
// the hand-over of existing objects and before the final COMMIT, so it runs in the
// script's one transaction.
func TestRolesScriptGuardsTheRedactionFunction(t *testing.T) {
	t.Parallel()

	script := code(rolesScript(t))
	blocks, rest := splitDoBlocks(script)
	if strings.Contains(rest, redactionFunctionName) {
		t.Errorf("scripts/roles.sql names %s outside a DO block that first checks it exists", redactionFunctionName)
	}
	guard := regexp.MustCompile(`(?is)IF\s+to_regprocedure\(\s*'public\.anbud_redact_raw_document\(bigint,\s*text,\s*text\)'\s*\)\s+IS\s+NOT\s+NULL\s+THEN`)
	var mine []string
	for _, block := range blocks {
		if strings.Contains(block, redactionFunctionName) {
			mine = append(mine, block)
			if !guard.MatchString(block) {
				t.Error("the block that names the redaction function does not start with IF to_regprocedure(...) IS NOT NULL THEN")
			}
		}
	}
	if len(mine) != 1 {
		t.Fatalf("scripts/roles.sql has %d blocks for the redaction function, want exactly one", len(mine))
	}

	// The statements written out: the one grant is EXECUTE to the DBA group; the revokes are ALL
	// from PUBLIC and the application, and the grant option of the group (with what was passed
	// on through it). Nothing else is granted or revoked in the open.
	onlyDBA := regexp.MustCompile(`(?i)^GRANT EXECUTE ON FUNCTION public\.` + redactionSignature + ` TO anbud_dba$`)
	revokeAll := regexp.MustCompile(`(?i)^REVOKE ALL ON FUNCTION public\.` + redactionSignature + ` FROM PUBLIC, anbud_app$`)
	revokeOption := regexp.MustCompile(`(?i)^REVOKE GRANT OPTION FOR EXECUTE ON FUNCTION public\.` + redactionSignature + ` FROM anbud_dba CASCADE$`)
	var grants, revokesAll, revokesOption int
	for _, stmt := range sqlStatementsOfBlocks(mine) {
		upper := strings.ToUpper(stmt)
		switch {
		case strings.HasPrefix(upper, "GRANT "):
			grants++
			if !onlyDBA.MatchString(stmt) {
				t.Errorf("the script grants %q, want EXECUTE to anbud_dba only", stmt)
			}
		case strings.HasPrefix(upper, "REVOKE "):
			switch {
			case revokeAll.MatchString(stmt):
				revokesAll++
			case revokeOption.MatchString(stmt):
				revokesOption++
			default:
				t.Errorf("the script revokes %q, want ALL from PUBLIC and anbud_app, or the grant option of anbud_dba", stmt)
			}
		}
	}
	if grants != 1 || revokesAll != 1 || revokesOption != 1 {
		t.Errorf("the block has %d grants, %d revokes of ALL and %d revokes of the grant option, want one of each", grants, revokesAll, revokesOption)
	}

	// The statements written for every other grantee are made in a loop over the function's own
	// ACL, so a grant to a role the script has never heard of is found, and the loop spares the owner
	// and the DBA group and nobody else.
	block := strings.Join(strings.Fields(mine[0]), " ")
	for _, want := range []string{
		"aclexplode(p.proacl)",
		"a.grantee <> p.proowner",
		"a.grantee <> 'anbud_dba'::regrole",
		"REVOKE ALL ON FUNCTION public.anbud_redact_raw_document(bigint, text, text) FROM %s CASCADE",
	} {
		if !strings.Contains(block, want) {
			t.Errorf("the block for the redaction function does not contain %q: it must revoke from every other direct grantee", want)
		}
	}

	at := strings.Index(script, redactionFunctionName)
	if handover, commit := strings.Index(script, "OWNER TO anbud_migrator"), strings.LastIndex(script, "COMMIT;"); at < handover || at > commit {
		t.Error("the block must come after the hand-over of existing objects and before the final COMMIT")
	}
}

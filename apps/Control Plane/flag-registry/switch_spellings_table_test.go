package flagregistry

import (
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"testing"
)

// spellingTable is one Model Plane service's exact-switch table: a Rust module whose EXACT_SWITCHES lists each
// switch the service reads by an exact literal, with the value that turns it on. Each table is pinned to its
// service's readers by a Rust test, so reading the tables here ties the registry to the readers without a
// second copy of the names.
type spellingTable struct {
	service string
	path    string
}

var spellingTables = []spellingTable{
	{"model-gateway", "apps/Model Plane/rust/services/model-gateway/src/switch_spellings.rs"},
	{"execution-core", "apps/Model Plane/rust/services/execution-core/src/switch_spellings.rs"},
	{"session-core", "apps/Model Plane/rust/services/session-core/src/switch_spellings.rs"},
	{"inference-core", "apps/Model Plane/rust/services/inference-core/src/switch_spellings.rs"},
}

// exactEntry is one EXACT_SWITCHES row: the variable, and the value that turns it on.
type exactEntry struct {
	name    string
	literal string
}

var (
	// exactTuple matches one row, ("NAME", "value") or (path::CONST, "value"), as the tables write them. A path is
	// module names joined by ::, with or without the crate:: prefix. A row may break after its value, before its
	// closing parenthesis, at the trailing comma that rustfmt writes there.
	exactTuple = regexp.MustCompile(`\(\s*(?:"([A-Z][A-Z0-9_]*)"|((?:[a-z_][a-z0-9_]*::)+[A-Z][A-Z0-9_]*))\s*,\s*"([^"]*)"\s*,?\s*\)`)
	// tupleOpener matches each opening parenthesis of a table, so a row the pattern above does not read is counted.
	tupleOpener = regexp.MustCompile(`\(`)
	// anyCaseException matches a DOCUMENTED_EXCEPTIONS row whose reason says the switch is read true in any
	// case. Each row is laid out over several lines, so the pattern runs across them.
	anyCaseException = regexp.MustCompile(`"([A-Z][A-Z0-9_]*)",\s*"true in any case`)
)

// readRepoText returns a repository file as text with LF line endings.
func readRepoText(t *testing.T, rel string) string {
	t.Helper()
	data, err := os.ReadFile(repoFile(rel))
	if err != nil {
		t.Fatalf("read %s: %v", rel, err)
	}
	return strings.ReplaceAll(string(data), "\r\n", "\n")
}

// tableBody returns the text of the constant named by marker, from its declaration to its closing "];", with
// the declaration's own "= &[" and its comment lines removed.
func tableBody(t *testing.T, rel, text, marker string) string {
	t.Helper()
	from := strings.Index(text, marker)
	if from < 0 {
		t.Fatalf("%s: no %s", rel, marker)
	}
	to := strings.Index(text[from:], "];")
	if to < 0 {
		t.Fatalf("%s: %s is not closed by ];", rel, marker)
	}
	block := text[from : from+to]
	open := strings.Index(block, "= &[")
	if open < 0 {
		t.Fatalf("%s: %s is not a slice literal", rel, marker)
	}
	var kept []string
	for _, line := range strings.Split(block[open+len("= &["):], "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), "//") {
			continue
		}
		kept = append(kept, line)
	}
	return strings.Join(kept, "\n")
}

// tableConstants returns the constants of the service that holds a table, resolved as the reader scan resolves them.
// A table names some switches through a constant, such as crate::auth::SERVICE_FLAG_AI_PII, and the constant's value
// is the variable's name. Each service reads its own sources, and test items and comments are left out of them, as
// the reader scan leaves them out.
func tableConstants(t *testing.T, tbl spellingTable) *constTable {
	t.Helper()
	srcDir := path.Dir(tbl.path)
	dir := repoFile(srcDir)
	var files []sourceFile
	err := filepath.WalkDir(dir, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() || !strings.HasSuffix(p, ".rs") {
			return nil
		}
		inSrc, err := filepath.Rel(dir, p)
		if err != nil {
			return err
		}
		files = append(files, readSourceFile(t, p, path.Join(srcDir, filepath.ToSlash(inSrc))))
		return nil
	})
	if err != nil {
		t.Fatalf("walk %s: %v", srcDir, err)
	}
	return buildConstTable(nil, productionSources(files))
}

// exactRow is one row of a table before its name is resolved: a quoted name or the path to a constant, and the
// literal that turns the switch on.
type exactRow struct {
	quoted, constant, literal string
}

// parseExactRows returns the rows of a table body in order, and how many opening parentheses the body holds. A
// body with more parentheses than rows holds a row this reader does not take.
func parseExactRows(body string) ([]exactRow, int) {
	var rows []exactRow
	for _, m := range exactTuple.FindAllStringSubmatch(body, -1) {
		rows = append(rows, exactRow{quoted: m[1], constant: m[2], literal: m[3]})
	}
	return rows, len(tupleOpener.FindAllString(body, -1))
}

// readExactTable returns the rows of a table's EXACT_SWITCHES in order. Every row must parse: a row written in a
// shape this reader does not know fails here, instead of being skipped and passing silently. A constant is resolved
// through the table's own service.
func readExactTable(t *testing.T, tbl spellingTable, consts *constTable) []exactEntry {
	t.Helper()
	body := tableBody(t, tbl.path, readRepoText(t, tbl.path), "pub const EXACT_SWITCHES")
	rows, openers := parseExactRows(body)
	if openers != len(rows) {
		t.Fatalf("%s: %d rows parse but %d look like rows; the table has a shape this reader does not take", tbl.path, len(rows), openers)
	}
	out := make([]exactEntry, 0, len(rows))
	for _, r := range rows {
		name := r.quoted
		if name == "" {
			v, ok := consts.resolve(tbl.path, r.constant)
			if !ok {
				t.Fatalf("%s: %s is a constant this test does not resolve to one value", tbl.path, r.constant)
			}
			name = v
		}
		out = append(out, exactEntry{name: name, literal: r.literal})
	}
	if len(out) == 0 {
		t.Fatalf("%s: EXACT_SWITCHES has no rows", tbl.path)
	}
	return out
}

// anyCaseNames returns the switches the gateway's DOCUMENTED_EXCEPTIONS lists as read true in any case.
func anyCaseNames(t *testing.T, tbl spellingTable) []string {
	t.Helper()
	body := tableBody(t, tbl.path, readRepoText(t, tbl.path), "pub const DOCUMENTED_EXCEPTIONS")
	var out []string
	for _, m := range anyCaseException.FindAllStringSubmatch(body, -1) {
		out = append(out, m[1])
	}
	return out
}

// checkExactEntry checks one registry row against the literal its service's table names for it.
func checkExactEntry(t *testing.T, tbl spellingTable, f Flag, literal string) {
	t.Helper()
	switch f.Kind {
	case KindSwitch:
		if literal != "true" && literal != "1" {
			t.Errorf("%s lists %s with %q, which no exact switch reader is written for", tbl.service, f.Name, literal)
			return
		}
		if f.OnLiteral != literal || f.OnLiteralAnyCase != "" {
			t.Errorf("%s reads %s by the exact literal %s, but its registry row has onLiteral %q and onLiteralAnyCase %q; the row needs onLiteral %s",
				tbl.service, f.Name, literal, f.OnLiteral, f.OnLiteralAnyCase, literal)
		}
	case KindChoice:
		if !slices.Contains(f.Values, literal) {
			t.Errorf("%s lists %s with %q, which is not one of its values %v", tbl.service, f.Name, literal, f.Values)
		}
		if f.ChoiceRead != ChoiceReadExact {
			t.Errorf("%s reads %s exactly as %q, but its registry row has choiceRead %q; the row needs choiceRead exact",
				tbl.service, f.Name, literal, f.ChoiceRead)
		}
	default:
		t.Errorf("%s lists %s as an exact switch, but its registry row is a %s flag", tbl.service, f.Name, f.Kind)
	}
}

// tableGaps returns the table names that the registry does not describe: a name with no row and no place on the
// registry's unregistered baseline. The baseline is the visible list of such names, which may only shrink, so a name
// on it is a known gap; any other name a table adds without a row is a new one.
func tableGaps(entries []exactEntry, reg *Registry) []string {
	baseline := append(reg.Unregistered(), reg.UnregisteredSwitches()...)
	var gaps []string
	for _, e := range entries {
		if _, ok := reg.Flag(e.name); ok {
			continue
		}
		if !slices.Contains(baseline, e.name) {
			gaps = append(gaps, e.name)
		}
	}
	return gaps
}

// TestSwitchSpellingTablesAgreeWithTheRegistry reads the Model Plane exact-switch tables and checks every registry
// row they name. A switch a table reads by the exact literal true or 1 must declare that literal as onLiteral; a
// choice a table reads exactly must declare choiceRead exact; the switches the gateway lists as true in any case
// must declare onLiteralAnyCase, and no other row may. A name with no registry row has no typed read to disagree
// with, so its row is not checked; it must be on the unregistered baseline (see tableGaps), so a switch a table adds
// without a row fails here.
func TestSwitchSpellingTablesAgreeWithTheRegistry(t *testing.T) {
	reg := loadShipped(t)
	for _, tbl := range spellingTables {
		entries := readExactTable(t, tbl, tableConstants(t, tbl))
		if tbl.service == "model-gateway" && len(entries) < 100 {
			// The gateway's table lists well over a hundred switches; a parse that finds far fewer has lost rows.
			t.Fatalf("%s: found %d EXACT_SWITCHES rows, want more than 100", tbl.path, len(entries))
		}
		for _, e := range entries {
			if f, ok := reg.Flag(e.name); ok {
				checkExactEntry(t, tbl, f, e.literal)
			}
		}
		if gaps := tableGaps(entries, reg); len(gaps) > 0 {
			t.Errorf("%s lists %d switches with neither a registry row nor an unregistered baseline entry: %v; add a row for each to flag-registry.json", tbl.service, len(gaps), gaps)
		}
	}

	gateway := spellingTables[0]
	anyCase := anyCaseNames(t, gateway)
	if !slices.Contains(anyCase, "MODEL_GATEWAY_REVIEW_RUN_ENABLED") {
		t.Fatalf("%s: DOCUMENTED_EXCEPTIONS does not list MODEL_GATEWAY_REVIEW_RUN_ENABLED as true in any case; the reader is out of step", gateway.path)
	}
	for _, name := range anyCase {
		if f, ok := reg.Flag(name); ok && (f.OnLiteralAnyCase != "true" || f.OnLiteral != "") {
			t.Errorf("%s is read true in any case by the gateway, but its registry row has onLiteral %q and onLiteralAnyCase %q; the row needs onLiteralAnyCase true",
				name, f.OnLiteral, f.OnLiteralAnyCase)
		}
	}
	for _, f := range reg.Flags() {
		if f.OnLiteralAnyCase != "" && !slices.Contains(anyCase, f.Name) {
			t.Errorf("%s declares onLiteralAnyCase, but the gateway's DOCUMENTED_EXCEPTIONS does not list it as true in any case", f.Name)
		}
	}
}

// TestExactTableRowsParseInEachShape pins the row shapes the tables use: a quoted name, a constant path with or without
// the crate:: prefix and with one or more modules, and a row broken over lines with the trailing comma rustfmt writes.
// A parenthesis that does not open a row this reader takes is counted, so the table fails rather than losing it.
func TestExactTableRowsParseInEachShape(t *testing.T) {
	body := `
    ("FINETUNE_ENABLED", "true"),
    (crate::auth::DEV_BYPASS_ENV, "true"),
    (
        crate::doc_indexed_consumer::ALLOW_UNVERIFIED_LEGACY_EVENTS_ENV,
        "1",
    ),
    (crate::provider::anthropic::TIERED_SYSTEM_ENV, "true"),
    (auth::SERVICE_FLAG_AI_PII, "true"),
`
	rows, openers := parseExactRows(body)
	want := []exactRow{
		{quoted: "FINETUNE_ENABLED", literal: "true"},
		{constant: "crate::auth::DEV_BYPASS_ENV", literal: "true"},
		{constant: "crate::doc_indexed_consumer::ALLOW_UNVERIFIED_LEGACY_EVENTS_ENV", literal: "1"},
		{constant: "crate::provider::anthropic::TIERED_SYSTEM_ENV", literal: "true"},
		{constant: "auth::SERVICE_FLAG_AI_PII", literal: "true"},
	}
	if !slices.Equal(rows, want) || openers != len(want) {
		t.Errorf("parseExactRows = %+v with %d openers, want %+v with %d", rows, openers, want, len(want))
	}
	// A row whose name is a bare identifier has no path for the reader to resolve, so the parser reads no row for it,
	// and its parenthesis is counted: readExactTable then fails the table by name.
	rows, openers = parseExactRows(`(DEV_BYPASS_ENV, "true"),`)
	if len(rows) != 0 || openers != 1 {
		t.Errorf("a bare-identifier row parsed as %+v with %d openers, want no rows and one opener", rows, openers)
	}
}

// TestTableGapsAreNamesWithNoRowAndNoBaselineEntry pins the gap rule. A table name is a gap only when the registry
// has no row for it and does not list it on either unregistered baseline; a registered name is never a gap, and a
// baseline name is a known one.
func TestTableGapsAreNamesWithNoRowAndNoBaselineEntry(t *testing.T) {
	reg, err := Load(strings.NewReader(`{"schema":"verevon.flag-registry/v1",` +
		`"flags":[{"name":"ROW_SWITCH","kind":"switch","onLiteral":"true","owner":"model-plane","source":"code",` +
		`"default":"false","purpose":"Fixture. Only the exact literal true turns it on.","codePath":"x/y.rs"}],` +
		`"unregistered":["ZZ_UNREGISTERED_ENABLED"],"unregisteredSwitches":["ZZ_SWITCH_BASELINE"]}`))
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	entries := []exactEntry{
		{name: "ROW_SWITCH", literal: "true"},
		{name: "ZZ_UNREGISTERED_ENABLED", literal: "true"},
		{name: "ZZ_SWITCH_BASELINE", literal: "true"},
		{name: "ZZ_NO_ROW", literal: "true"},
	}
	if got, want := tableGaps(entries, reg), []string{"ZZ_NO_ROW"}; !slices.Equal(got, want) {
		t.Errorf("tableGaps = %v, want %v", got, want)
	}
}

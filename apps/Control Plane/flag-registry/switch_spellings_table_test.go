package flagregistry

import (
	"os"
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

// gatewaySrc holds the model-gateway modules. The gateway's table names some switches through constants, such as
// auth::SERVICE_FLAG_AI_PII, and their values are read from these files.
const gatewaySrc = "apps/Model Plane/rust/services/model-gateway/src"

// exactEntry is one EXACT_SWITCHES row: the variable, and the value that turns it on.
type exactEntry struct {
	name    string
	literal string
}

var (
	// exactTuple matches one row, ("NAME", "value") or (module::CONST, "value"), as the tables write them.
	exactTuple = regexp.MustCompile(`\(\s*(?:"([A-Z][A-Z0-9_]*)"|([a-z_]+::[A-Z][A-Z0-9_]*))\s*,\s*"([^"]*)"\s*\)`)
	// tupleOpener matches the start of any row, so a row the pattern above does not read is counted.
	tupleOpener = regexp.MustCompile(`\(\s*(?:"|[a-z_]+::)`)
	// rustStringConst matches a string constant, pub const NAME: &str = "VALUE";
	rustStringConst = regexp.MustCompile(`const\s+([A-Z][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([^"]*)"\s*;`)
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

// gatewayConstants maps module::CONST to the variable name the model-gateway source gives it.
func gatewayConstants(t *testing.T) map[string]string {
	t.Helper()
	dir := repoFile(gatewaySrc)
	files, err := os.ReadDir(dir)
	if err != nil {
		t.Fatalf("read %s: %v", gatewaySrc, err)
	}
	out := make(map[string]string)
	for _, f := range files {
		if f.IsDir() || !strings.HasSuffix(f.Name(), ".rs") {
			continue
		}
		data, err := os.ReadFile(filepath.Join(dir, f.Name()))
		if err != nil {
			t.Fatalf("read %s: %v", f.Name(), err)
		}
		module := strings.TrimSuffix(f.Name(), ".rs")
		for _, m := range rustStringConst.FindAllStringSubmatch(strings.ReplaceAll(string(data), "\r\n", "\n"), -1) {
			out[module+"::"+m[1]] = m[2]
		}
	}
	return out
}

// readExactTable returns the rows of a table's EXACT_SWITCHES in order. Every row must parse: a row written in a
// shape this reader does not know fails here, instead of being skipped and passing silently.
func readExactTable(t *testing.T, tbl spellingTable, consts map[string]string) []exactEntry {
	t.Helper()
	body := tableBody(t, tbl.path, readRepoText(t, tbl.path), "pub const EXACT_SWITCHES")
	rows := exactTuple.FindAllStringSubmatch(body, -1)
	if openers := tupleOpener.FindAllString(body, -1); len(openers) != len(rows) {
		t.Fatalf("%s: %d rows parse but %d look like rows; the table has a shape this reader does not take", tbl.path, len(rows), len(openers))
	}
	out := make([]exactEntry, 0, len(rows))
	for _, m := range rows {
		name := m[1]
		if name == "" {
			name = consts[m[2]]
			if name == "" {
				t.Fatalf("%s: %s is a constant this test does not resolve", tbl.path, m[2])
			}
		}
		out = append(out, exactEntry{name: name, literal: m[3]})
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

// TestSwitchSpellingTablesAgreeWithTheRegistry reads the Model Plane exact-switch tables and checks every registry
// row they name. A switch a table reads by the exact literal true or 1 must declare that literal as onLiteral; a
// choice a table reads exactly must declare choiceRead exact; the switches the gateway lists as true in any case
// must declare onLiteralAnyCase, and no other row may. A name with no registry row has no typed read to disagree
// with, so it is not checked.
func TestSwitchSpellingTablesAgreeWithTheRegistry(t *testing.T) {
	reg := loadShipped(t)
	consts := gatewayConstants(t)
	for _, tbl := range spellingTables {
		entries := readExactTable(t, tbl, consts)
		if tbl.service == "model-gateway" && len(entries) < 100 {
			// The gateway's table lists well over a hundred switches; a parse that finds far fewer has lost rows.
			t.Fatalf("%s: found %d EXACT_SWITCHES rows, want more than 100", tbl.path, len(entries))
		}
		for _, e := range entries {
			if f, ok := reg.Flag(e.name); ok {
				checkExactEntry(t, tbl, f, e.literal)
			}
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

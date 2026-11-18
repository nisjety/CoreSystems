package flagregistry

import (
	"fmt"
	"io/fs"
	"maps"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"testing"
)

// The reader cross-check. A registry row for a switch that a Model Plane service reads by the exact
// literal true, or 1, must declare that literal as onLiteral. The spelling tables pin four Rust services
// by name; this check reads every Go service and every Rust crate and service, so a reader added anywhere
// fails until its row says so.
//
// Every reader the scan finds must name its variable, by a literal or by a string constant. A reader it
// cannot name fails the test, so a shape the scan does not recognise is never skipped. The shapes are:
//   - Go: getenv("NAME") or os.Getenv("NAME") compared with "true" or "1", and lookup(NAME) the same way.
//   - Go helpers: a func(p string) bool whose body returns p compared with "true" or "1", called with an
//     environment read as its argument.
//   - Rust: switch_from, and every function that calls one of the readers, or compares its value with
//     the literal, bare or inside Some(...) or Ok(...). An environment read passed to a reader names the
//     variable. An environment read compared with the literal in its statement, or bound to a local that
//     a reader takes later in the same function, is exact too.
//
// A call resolves to the functions of its own file with its name, and to every function of that name in the
// tree only when the file has none, and only when they all read the same literal. A constant resolves through
// its module, a `use ... as` alias, the scope that declares it, or the tree, and each step needs one value.
//
// A name with no registry row has no typed read to disagree with, so it is not checked.

// literalReaderRoots are the Model Plane trees the scan reads. Rust is read only under each crate's and
// service's src directory, so fixtures in other directories stay out.
var literalReaderRoots = []string{
	"apps/Model Plane/go/services",
	"apps/Model Plane/rust/crates",
	"apps/Model Plane/rust/services",
}

// literalReaderFloor is the fewest variables the scan may find. A scan that finds fewer has lost a shape.
const literalReaderFloor = 100

// literalReaderPins are switches the scan must find. Each one exercises a shape the scan recognises, so a
// shape that stops matching fails its pin instead of dropping out of the check without a sign.
var literalReaderPins = []string{
	"AGENTIC_SUBSCRIPTION_ENABLED",   // Rust: from_env passes the reads through from_values and switch_value_is_on
	"APPROVAL_SCOPE_GRANTS_ENABLED",  // Rust: scope_grants_from wraps switch_from over a named constant
	"EVENT_TRIGGERS_ENABLED",         // Rust: switch_from over a named constant
	"EVENT_TRIGGER_FIRE_ENABLED",     // Go: os.Getenv compared with "true"
	"INFERENCE_ORG_OVERLAY",          // Rust: a wrapper over a named constant
	"MEMORY_GATE_CAPABILITY_WRITES",  // Go: lookup over a constant, compared with "true"
	"PROMOTE_ON_USE_DURABLE",         // Rust: an environment read compared with "true" in the same statement
	"PROMOTE_ON_USE_ENABLED",         // Rust: the same, over a literal name
	"PROVIDER_READ_DOCUMENT_URL",     // Rust: a wrapper compared with Some("true"), over a path-qualified constant
	"RESEARCH_RUNS_ENABLED",          // Rust: switch_from over a named constant
	"SESSION_THREAD_RECEIPT_ENFORCE", // Rust: a wrapper over switch_from, over a named constant
	"TOOL_POLICY_RULE_AUDIT_ENABLED", // Go: lookup over a constant, compared with "true"
}

// literalReaderSite is one exact read of a variable: the variable, the literal that turns it on, and where
// the read sits.
type literalReaderSite struct {
	name    string
	literal string
	where   string
}

// unnamedReader is an exact read whose variable the scan cannot resolve to one string constant.
type unnamedReader struct {
	where string
	ident string
}

// sourceFile is one file the scan reads: its repository path with forward slashes, and its text with LF
// line endings.
type sourceFile struct {
	rel  string
	text string
}

// The scan does not run a regexp over whole files. Go's regexp engine is too slow for patterns with no
// literal prefix across the Model Plane tree, so it finds keywords and calls with plain byte scans, and
// applies a small anchored regexp only where a keyword starts.
var (
	// goNameArg matches the single argument of a Go read: a literal name, or a constant a package may qualify.
	goNameArg = regexp.MustCompile(`^\s*(?:"([A-Z][A-Z0-9_]*)"|((?:[A-Za-z_]\w*\.)*[A-Za-z_]\w*))\s*$`)
	// goCompareAfter matches the comparison that makes a Go read exact.
	goCompareAfter = regexp.MustCompile(`^\s*==\s*"(true|1)"`)
	// goValueHelper matches a helper that returns its string parameter compared with the literal.
	goValueHelper = regexp.MustCompile(`^func\s+(\w+)\(\s*(\w+)\s+string\s*\)\s+bool\s*\{\s*return\s+(\w+)\s*==\s*"(true|1)"\s*\}`)
	// goConstLine matches a string constant declared on one line.
	goConstLine = regexp.MustCompile(`^const\s+([A-Za-z_]\w*)(?:\s+string)?\s*=\s*"([A-Z][A-Z0-9_]*)"`)
	// goConstItem matches one string constant inside a const block.
	goConstItem = regexp.MustCompile(`(?m)^\s*([A-Za-z_]\w*)(?:\s+string)?\s*=\s*"([A-Z][A-Z0-9_]*)"`)
	// rustConstLine matches a string constant of type &str, which may be an associated constant.
	rustConstLine = regexp.MustCompile(`^const\s+([A-Z][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*"([A-Z][A-Z0-9_]*)"\s*;`)
	// rustUseAlias matches a `use` rename of a constant: parts::ENABLED_VAR as CHAT_VAR.
	rustUseAlias = regexp.MustCompile(`(?:(\w+)::)?([A-Z][A-Z0-9_]*)\s+as\s+([A-Z][A-Z0-9_]*)\b`)
	// rustFnName matches the start of a function item, through its name.
	rustFnName = regexp.MustCompile(`^fn\s+(\w+)`)
	// rustEnvRead matches an environment read, env::var or a get closure, whose variable is a literal or a
	// constant that a path may qualify.
	rustEnvRead = regexp.MustCompile(`^(?:var|get)\(\s*(?:"([A-Z][A-Z0-9_]*)"|((?:[A-Za-z_]\w*::)*[A-Z][A-Z0-9_]*))\s*\)`)
	// rustCompareAt matches a comparison with the literal true or 1, of the value itself or of Some or Ok of it.
	rustCompareAt = regexp.MustCompile(`^==\s*(?:Some\(\s*|Ok\(\s*)?"(true|1)"`)
	// rustCompareAfter matches such a comparison later in the same statement, before a brace or a semicolon.
	rustCompareAfter = regexp.MustCompile(`^[^;{}]{0,200}?==\s*(?:Some\(\s*|Ok\(\s*)?"(true|1)"`)
	// rustLetBinding matches the start of `let name = `, and of the path that may precede an environment read
	// on its right-hand side, at the end of the text before the read.
	rustLetBinding = regexp.MustCompile(`let\s+(?:mut\s+)?([a-z_][a-z0-9_]*)\s*(?::[^=;]*)?=\s*(?:[A-Za-z_]\w*::)*$`)
	// rustFnStart matches the start of a function item on its own line, which ends the scope of a local.
	rustFnStart = regexp.MustCompile(`(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s`)
)

// TestLiteralSwitchReadersInSourceAreRegistered reads the Model Plane source and checks every registry row
// for a switch that some reader in it reads by an exact literal. Such a row must declare that literal as
// onLiteral, and no row may declare a different literal or the wrong kind.
func TestLiteralSwitchReadersInSourceAreRegistered(t *testing.T) {
	reg := loadShipped(t)
	goFiles, rustFiles := readLiteralReaderSources(t)
	sites, unnamed := findLiteralReaders(goFiles, rustFiles)
	for _, u := range unnamed {
		t.Errorf("%s reads %s by an exact literal, but the scan cannot resolve %s to one string constant; read the variable by a literal or a constant the scan can see", u.where, u.ident, u.ident)
	}
	byName := make(map[string][]literalReaderSite)
	for _, s := range sites {
		byName[s.name] = append(byName[s.name], s)
	}
	for _, name := range literalReaderPins {
		if len(byName[name]) == 0 {
			t.Errorf("%s is pinned as an exact reader, but the scan finds no reader for it", name)
		}
	}
	if len(byName) < literalReaderFloor {
		t.Fatalf("the scan finds %d variables read by an exact literal, want at least %d; a shape may have stopped matching", len(byName), literalReaderFloor)
	}
	for _, name := range slices.Sorted(maps.Keys(byName)) {
		f, ok := reg.Flag(name)
		if !ok {
			continue
		}
		first := byName[name][0]
		if f.Kind != KindSwitch {
			t.Errorf("%s is read by the exact literal %s at %s, but its registry row is a %s flag", name, first.literal, first.where, f.Kind)
			continue
		}
		for _, s := range byName[name] {
			if f.OnLiteral != s.literal || f.OnLiteralAnyCase != "" {
				t.Errorf("%s is read by the exact literal %s at %s, but its registry row has onLiteral %q and onLiteralAnyCase %q; the row needs onLiteral %s",
					name, s.literal, s.where, f.OnLiteral, f.OnLiteralAnyCase, s.literal)
				break
			}
		}
	}
}

// TestLiteralReaderScanRecognisesEachShape runs the scan on one small source per shape, so a change that
// drops a shape, or a rule that resolves a name to the wrong function, fails here by name.
func TestLiteralReaderScanRecognisesEachShape(t *testing.T) {
	goSrc := `package x

const (
	getEnvName = "GO_BLOCK_ENABLED"
)

const lookupName = "GO_LOOKUP_ENABLED"

func direct() bool { return os.Getenv("GO_LITERAL_ENABLED") == "true" }

func viaLookup(lookup func(string) string) bool { return lookup(lookupName) == "true" }

func one(value string) bool {
	return value == "1"
}

func gate() bool { return one(os.Getenv("GO_GATE_ENABLED")) }

func helper(value string) bool {
	return value == "true"
}

func viaHelper() bool { return helper(os.Getenv(getEnvName)) }

func parsed() bool { return os.Getenv("GO_PARSED_ENABLED") != "" }

func unnamed(getenv func(string) string, name string) bool { return getenv(name) == "true" }
`
	rustLib := `pub const ENABLED_ENV: &str = "RUST_SWITCH_ENABLED";

pub fn gate(raw: Option<&str>) -> bool {
    switch_from(raw)
}

pub fn wraps() -> bool {
    gate(std::env::var(ENABLED_ENV).ok().as_deref())
}

pub fn from_values(enabled: Option<&str>) -> bool {
    gate(enabled)
}

pub fn from_env() -> bool {
    from_values(std::env::var(ENABLED_ENV).ok().as_deref())
}

pub fn map_read() -> bool {
    std::env::var("RUST_MAP_ENABLED")
        .map(|v| v == "true")
        .unwrap_or(false)
}

pub fn plain(raw: Option<&str>) -> bool {
    raw.is_some()
}

pub fn unrelated() -> bool {
    plain(std::env::var("RUST_UNRELATED_ENABLED").ok().as_deref())
}
`
	parseA := `pub fn parse(raw: Option<&str>) -> bool {
    raw == Some("true")
}

pub fn on_read() -> bool {
    parse(std::env::var("PARSE_ON_ENABLED").ok().as_deref())
}
`
	parseB := `pub fn parse(value: Option<&str>) -> u8 {
    match value {
        Some("x") => 1,
        _ => 0,
    }
}

pub fn off_read() -> u8 {
    parse(std::env::var("PARSE_OFF_ENABLED").ok().as_deref())
}
`
	partsRs := `pub const ENABLED_VAR: &str = "ALIAS_TARGET_ENABLED";
`
	otherRs := `pub const ENABLED_VAR: &str = "OTHER_ENABLED";
`
	aliasUser := `use crate::vision::{parts::ENABLED_VAR as CHAT_ALIAS, InvokeAttachment};

pub fn aliased() -> bool {
    switch_from(get(CHAT_ALIAS).as_deref())
}
`
	localRs := `pub fn speed() -> bool {
    let raw = std::env::var("LOCAL_BOUND_ENABLED").ok();
    switch_from(raw.as_deref())
}

pub fn unbound() -> bool {
    let raw = std::env::var("LOCAL_UNUSED_ENABLED").ok();
    raw.is_some()
}
`
	selfGate := `pub struct Gate;

impl Gate {
    const ENV: &'static str = "SELF_GATE_ENABLED";

    fn from_env() -> bool {
        switch_from(std::env::var(Self::ENV).ok().as_deref())
    }
}
`
	const src = "apps/Model Plane/rust/services/x/src/"
	goFiles := []sourceFile{{rel: "apps/Model Plane/go/services/x/x.go", text: goSrc}}
	rustFiles := []sourceFile{
		{rel: src + "lib.rs", text: rustLib},
		{rel: src + "parse_a.rs", text: parseA},
		{rel: src + "parse_b.rs", text: parseB},
		{rel: src + "vision/parts.rs", text: partsRs},
		{rel: src + "vision/other.rs", text: otherRs},
		{rel: src + "alias_user.rs", text: aliasUser},
		{rel: src + "gate.rs", text: selfGate},
		{rel: src + "local.rs", text: localRs},
	}
	sites, unnamed := findLiteralReaders(goFiles, rustFiles)

	var got []string
	for _, s := range sites {
		got = append(got, s.name+" "+s.literal)
	}
	slices.Sort(got)
	got = slices.Compact(got)
	want := []string{
		"ALIAS_TARGET_ENABLED true",
		"GO_BLOCK_ENABLED true",
		"GO_GATE_ENABLED 1",
		"GO_LITERAL_ENABLED true",
		"GO_LOOKUP_ENABLED true",
		"LOCAL_BOUND_ENABLED true",
		"PARSE_ON_ENABLED true",
		"RUST_MAP_ENABLED true",
		"RUST_SWITCH_ENABLED true",
		"SELF_GATE_ENABLED true",
	}
	if !slices.Equal(got, want) {
		t.Errorf("sites = %v, want %v", got, want)
	}
	if len(unnamed) != 1 || unnamed[0].ident != "name" {
		t.Errorf("unnamed = %+v, want the one reader that passes a parameter", unnamed)
	}
}

// readLiteralReaderSources reads every Go service file that is not a test, and every Rust file under a src
// directory, from the literal reader roots.
func readLiteralReaderSources(t *testing.T) (goFiles, rustFiles []sourceFile) {
	t.Helper()
	for _, root := range literalReaderRoots {
		base := repoFile(root)
		err := filepath.WalkDir(base, func(p string, d fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if d.IsDir() {
				return nil
			}
			inRoot, err := filepath.Rel(base, p)
			if err != nil {
				return err
			}
			rel := path.Join(root, filepath.ToSlash(inRoot))
			switch {
			case strings.HasSuffix(rel, "_test.go") || strings.Contains(rel, "/testdata/"):
			case strings.HasSuffix(rel, ".go"):
				goFiles = append(goFiles, readSourceFile(t, p, rel))
			case strings.HasSuffix(rel, ".rs") && strings.Contains(rel, "/src/"):
				rustFiles = append(rustFiles, readSourceFile(t, p, rel))
			}
			return nil
		})
		if err != nil {
			t.Fatalf("walk %s: %v", root, err)
		}
	}
	return goFiles, rustFiles
}

func readSourceFile(t *testing.T, p, rel string) sourceFile {
	t.Helper()
	data, err := os.ReadFile(p)
	if err != nil {
		t.Fatalf("read %s: %v", rel, err)
	}
	return sourceFile{rel: rel, text: strings.ReplaceAll(string(data), "\r\n", "\n")}
}

// findLiteralReaders returns every exact read in the sources, and the reads whose variable it cannot name.
func findLiteralReaders(goFiles, rustFiles []sourceFile) ([]literalReaderSite, []unnamedReader) {
	consts := buildConstTable(goFiles, rustFiles)
	var sites []literalReaderSite
	var unnamed []unnamedReader
	// emit records one read at index at of text. The variable is the literal lit, or else the constant
	// ident, resolved from the scope that the read sits in.
	emit := func(file, scope, text string, at int, lit, ident, literal string) {
		where := fmt.Sprintf("%s:%d", file, strings.Count(text[:at], "\n")+1)
		name := lit
		if name == "" {
			v, ok := consts.resolve(scope, ident)
			if !ok {
				unnamed = append(unnamed, unnamedReader{where: where, ident: ident})
				return
			}
			name = v
		}
		sites = append(sites, literalReaderSite{name: name, literal: literal, where: where})
	}

	// Go: a read of a variable by getenv, os.Getenv or lookup, compared with the literal.
	for _, f := range goFiles {
		scope := path.Dir(f.rel)
		for _, r := range goEnvReads(f.text, "Getenv", "getenv", "lookup") {
			if cm := goCompareAfter.FindStringSubmatch(f.text[r.end:min(r.end+64, len(f.text))]); cm != nil {
				emit(f.rel, scope, f.text, r.at, r.lit, r.ident, cm[1])
			}
		}
	}

	// Go: a helper that returns its parameter compared with the literal, called with an environment read.
	helpers := goValueHelpers(goFiles)
	for _, f := range goFiles {
		dir := path.Dir(f.rel)
		calls := identCalls(f.text)
		for _, name := range slices.Sorted(maps.Keys(calls)) {
			lit, ok := goHelperLiteral(helpers, dir, name)
			if !ok {
				continue
			}
			for _, c := range calls[name] {
				args, ok := callArgs(f.text, c.open)
				if !ok {
					continue
				}
				for _, r := range goEnvReads(args, "Getenv", "getenv") {
					emit(f.rel, dir, f.text, c.open+1+r.at, r.lit, r.ident, lit)
				}
			}
		}
	}

	// Rust: the readers are switch_from and every function that reaches it, or compares with the literal.
	var fns []rustFn
	for _, f := range rustFiles {
		fns = append(fns, rustFunctions(f.rel, f.text)...)
	}
	readers := rustExactReaders(fns)
	for _, f := range rustFiles {
		calls := identCalls(f.text)
		for _, name := range slices.Sorted(maps.Keys(calls)) {
			lit, ok := readers.resolve(f.rel, name)
			if !ok {
				continue
			}
			for _, c := range calls[name] {
				args, ok := callArgs(f.text, c.open)
				if !ok {
					continue
				}
				for _, r := range rustEnvReads(args) {
					// A read compared inside its own argument is not passed to the reader. The compare pass
					// below takes it.
					if rest := argRest(args[r.end:]); strings.Contains(rest, "==") || strings.Contains(rest, "!=") {
						continue
					}
					emit(f.rel, f.rel, f.text, c.open+1+r.at, r.lit, r.ident, lit)
				}
			}
		}
		// An environment read compared with the literal in the same statement, such as a map closure.
		for _, r := range rustEnvReads(f.text) {
			if cm := rustCompareAfter.FindStringSubmatch(f.text[r.end:min(r.end+200, len(f.text))]); cm != nil {
				emit(f.rel, f.rel, f.text, r.at, r.lit, r.ident, cm[1])
			}
		}
		// An environment read bound to a local, which a reader later in the same function takes as an argument.
		for _, r := range rustEnvReads(f.text) {
			m := rustLetBinding.FindStringSubmatch(f.text[max(0, r.at-160):r.at])
			if m == nil {
				continue
			}
			window := f.text[r.end:min(r.end+600, len(f.text))]
			if loc := rustFnStart.FindStringIndex(window); loc != nil {
				window = window[:loc[0]]
			}
			calls := identCalls(window)
			for _, name := range slices.Sorted(maps.Keys(calls)) {
				lit, ok := readers.resolve(f.rel, name)
				if !ok {
					continue
				}
				for _, c := range calls[name] {
					if args, ok := callArgs(window, c.open); ok && hasWord(args, m[1]) {
						emit(f.rel, f.rel, f.text, r.at, r.lit, r.ident, lit)
					}
				}
			}
		}
	}
	return sites, unnamed
}

// hasWord reports whether word occurs in s as a whole word, not inside a longer identifier and not after a dot.
func hasWord(s, word string) bool {
	for i := 0; ; {
		k := strings.Index(s[i:], word)
		if k < 0 {
			return false
		}
		at := i + k
		end := at + len(word)
		before := at == 0 || (!isIdentByte(s[at-1]) && s[at-1] != '.')
		after := end == len(s) || !isIdentByte(s[end])
		if before && after {
			return true
		}
		i = end
	}
}

// goHelper is a Go helper that returns its string parameter compared with a literal, with the directory that
// declares it.
type goHelper struct {
	dir, name, literal string
}

// goValueHelpers returns the helpers that return their string parameter compared with the literal.
func goValueHelpers(goFiles []sourceFile) []goHelper {
	var out []goHelper
	for _, f := range goFiles {
		for _, at := range wordStarts(f.text, "func ") {
			m := goValueHelper.FindStringSubmatch(f.text[at:min(at+400, len(f.text))])
			if m != nil && m[2] == m[3] {
				out = append(out, goHelper{dir: path.Dir(f.rel), name: m[1], literal: m[4]})
			}
		}
	}
	return out
}

// goHelperLiteral returns the literal a call to name from dir reads, when it is a helper. A call resolves to the
// helpers of its own directory with that name, and otherwise to every helper of the name, when they all read one
// literal.
func goHelperLiteral(helpers []goHelper, dir, name string) (string, bool) {
	var local, all []string
	for _, h := range helpers {
		if h.name != name {
			continue
		}
		all = append(all, h.literal)
		if h.dir == dir {
			local = append(local, h.literal)
		}
	}
	pool := local
	if len(pool) == 0 {
		pool = all
	}
	if len(pool) == 0 {
		return "", false
	}
	for _, l := range pool[1:] {
		if l != pool[0] {
			return "", false
		}
	}
	return pool[0], true
}

// goEnvReads returns each call to one of the named functions whose single argument is a variable name, such
// as os.Getenv("NAME") or lookup(name), in order. The read spans the call through its closing parenthesis.
func goEnvReads(text string, names ...string) []envRead {
	calls := identCalls(text)
	var out []envRead
	for _, name := range names {
		for _, c := range calls[name] {
			args, ok := callArgs(text, c.open)
			if !ok {
				continue
			}
			m := goNameArg.FindStringSubmatch(args)
			if m == nil {
				continue
			}
			out = append(out, envRead{at: c.start, end: c.open + len(args) + 2, lit: m[1], ident: m[2]})
		}
	}
	slices.SortFunc(out, func(a, b envRead) int { return a.at - b.at })
	return out
}

// envRead is one read of an environment variable: the span of the read in its text, and the variable, written
// as a literal or as the name of a constant.
type envRead struct {
	at, end    int
	lit, ident string
}

// rustEnvReads returns each env::var or get read in text, in order.
func rustEnvReads(text string) []envRead {
	var out []envRead
	for _, word := range []string{"var(", "get("} {
		for _, at := range wordStarts(text, word) {
			window := text[at:min(at+300, len(text))]
			m := rustEnvRead.FindStringSubmatchIndex(window)
			if m == nil {
				continue
			}
			out = append(out, envRead{at: at, end: at + m[1], lit: group(window, m, 1), ident: group(window, m, 2)})
		}
	}
	slices.SortFunc(out, func(a, b envRead) int { return a.at - b.at })
	return out
}

// argRest returns the text of one call argument from its start: the text up to the comma, or the closing
// parenthesis, that ends it, with nested brackets kept whole.
func argRest(s string) string {
	depth := 0
	for i := 0; i < len(s); i++ {
		switch s[i] {
		case '(', '[', '{':
			depth++
		case ')', ']', '}':
			if depth == 0 {
				return s[:i]
			}
			depth--
		case ',':
			if depth == 0 {
				return s[:i]
			}
		}
	}
	return s
}

// rustFn is one function item that has a body: the file that declares it, its name and the text between its
// braces.
type rustFn struct {
	file string
	name string
	body string
}

// rustFunctions returns the functions with a body in one file.
func rustFunctions(file, text string) []rustFn {
	var out []rustFn
	for _, at := range wordStarts(text, "fn ") {
		m := rustFnName.FindStringSubmatchIndex(text[at:min(at+200, len(text))])
		if m == nil {
			continue
		}
		nameEnd := at + m[1]
		open := strings.IndexByte(text[nameEnd:], '(')
		if open < 0 {
			continue
		}
		open += nameEnd
		closeParen := closingIndex(text, open, '(', ')')
		if closeParen < 0 {
			continue
		}
		next := strings.IndexAny(text[closeParen+1:], "{;")
		if next < 0 || text[closeParen+1+next] != '{' {
			continue // a declaration without a body
		}
		start := closeParen + 1 + next
		end := closingIndex(text, start, '{', '}')
		if end < 0 {
			continue
		}
		out = append(out, rustFn{file: file, name: text[at+m[2] : at+m[3]], body: text[start+1 : end]})
	}
	return out
}

// rustSeeds are the readers the scan knows when the tree does not define them. switch_from is the base reader
// of every Rust switch, defined in mp-contracts.
var rustSeeds = map[string]string{"switch_from": "true"}

// rustReaders are the Rust functions that read a switch exactly, and the literal each one reads. A call from a file
// resolves to the functions of that file with the call's name, and to every function of that name in the tree only
// when the file has none. It resolves to a literal only when every candidate reads that same literal, so a name that
// another file reads differently is never taken for an exact reader.
type rustReaders struct {
	fns    []rustFn
	byName map[string][]int
	lit    []string
}

func newRustReaders(fns []rustFn) *rustReaders {
	r := &rustReaders{fns: fns, byName: make(map[string][]int), lit: make([]string, len(fns))}
	for i, fn := range fns {
		r.byName[fn.name] = append(r.byName[fn.name], i)
	}
	return r
}

// resolve returns the literal a call to name from file reads exactly.
func (r *rustReaders) resolve(file, name string) (string, bool) {
	pool := r.byName[name]
	var local []int
	for _, i := range pool {
		if r.fns[i].file == file {
			local = append(local, i)
		}
	}
	if len(local) > 0 {
		pool = local
	}
	if len(pool) == 0 {
		lit, ok := rustSeeds[name]
		return lit, ok
	}
	lit := r.lit[pool[0]]
	if lit == "" {
		return "", false
	}
	for _, i := range pool[1:] {
		if r.lit[i] != lit {
			return "", false
		}
	}
	return lit, true
}

// rustExactReaders finds the exact readers by a fixed point: a function whose body reads a switch exactly, by a call
// to a reader or a comparison with the literal, reads the switch too. A wrapper, or a wrapper of a wrapper, therefore
// reads the switch it wraps.
func rustExactReaders(fns []rustFn) *rustReaders {
	r := newRustReaders(fns)
	for changed := true; changed; {
		changed = false
		for i, fn := range fns {
			if r.lit[i] != "" {
				continue
			}
			if lit, ok := r.bodyLiteral(fn); ok {
				r.lit[i] = lit
				changed = true
			}
		}
	}
	return r
}

// bodyLiteral reports the literal a function body reads exactly. Calls are tried in name order, so the answer does
// not depend on map order.
func (r *rustReaders) bodyLiteral(fn rustFn) (string, bool) {
	calls := identCalls(fn.body)
	for _, name := range slices.Sorted(maps.Keys(calls)) {
		if lit, ok := r.resolve(fn.file, name); ok {
			return lit, true
		}
	}
	for i := 0; ; {
		k := strings.Index(fn.body[i:], "==")
		if k < 0 {
			return "", false
		}
		at := i + k
		if m := rustCompareAt.FindStringSubmatch(fn.body[at:min(at+64, len(fn.body))]); m != nil {
			return m[1], true
		}
		i = at + 2
	}
}

// callSite is one call: where the called name starts, and where its open parenthesis is.
type callSite struct {
	start, open int
}

// identCalls returns the calls in text, by the name each one calls: an identifier directly followed by an
// open parenthesis, with only spaces between. A qualified call, such as os.Getenv(, is found by its last name.
func identCalls(text string) map[string][]callSite {
	calls := make(map[string][]callSite)
	for i := 0; i < len(text); {
		if !isIdentByte(text[i]) {
			i++
			continue
		}
		start := i
		for i < len(text) && isIdentByte(text[i]) {
			i++
		}
		j := i
		for j < len(text) && (text[j] == ' ' || text[j] == '\t' || text[j] == '\r' || text[j] == '\n') {
			j++
		}
		if j < len(text) && text[j] == '(' {
			name := text[start:i]
			calls[name] = append(calls[name], callSite{start: start, open: j})
		}
	}
	return calls
}

// isIdentByte reports whether c can be part of an identifier in Go or Rust.
func isIdentByte(c byte) bool {
	return c == '_' || '0' <= c && c <= '9' || 'a' <= c && c <= 'z' || 'A' <= c && c <= 'Z'
}

// wordStarts returns where each occurrence of word starts in text, skipping an occurrence that follows an
// identifier byte or a dot. So "get(" matches a call to get, and not target( or map.get(.
func wordStarts(text, word string) []int {
	var out []int
	for i := 0; ; {
		k := strings.Index(text[i:], word)
		if k < 0 {
			return out
		}
		at := i + k
		i = at + len(word)
		if at > 0 && (isIdentByte(text[at-1]) || text[at-1] == '.') {
			continue
		}
		out = append(out, at)
	}
}

// callArgs returns the text between the open parenthesis at open and the one that closes it, and false when
// none does.
func callArgs(text string, open int) (string, bool) {
	closeAt := closingIndex(text, open, '(', ')')
	if closeAt < 0 {
		return "", false
	}
	return text[open+1 : closeAt], true
}

// group returns the text of capture group g of a submatch index slice taken from text, or "" when the group
// did not match.
func group(text string, m []int, g int) string {
	if m[2*g] < 0 {
		return ""
	}
	return text[m[2*g]:m[2*g+1]]
}

// closingIndex returns the index of the bracket that closes the one at open, or -1 when the text does not
// close it.
func closingIndex(text string, open int, opener, closer byte) int {
	depth := 0
	for i := open; i < len(text); i++ {
		switch text[i] {
		case opener:
			depth++
		case closer:
			depth--
			if depth == 0 {
				return i
			}
		}
	}
	return -1
}

// constDecl is one string constant: the scope that declares it (a Go package directory or a Rust file), and its
// value.
type constDecl struct {
	scope, value string
}

// useAlias is a `use ... as` rename of a constant: the module it is imported from, and its name there.
type useAlias struct {
	module, name string
}

// constTable holds the string constants of the sources, by name, and the renames each Rust file makes.
type constTable struct {
	byName  map[string][]constDecl
	aliases map[string]map[string]useAlias
}

// scopeModule is the module name a scope's path gives: a Rust file's base name (a mod.rs takes its directory's
// name), or a Go package directory's base name.
func scopeModule(scope string) string {
	if strings.HasSuffix(scope, ".rs") {
		base := strings.TrimSuffix(path.Base(scope), ".rs")
		if base == "mod" || base == "lib" || base == "main" {
			return path.Base(path.Dir(scope))
		}
		return base
	}
	return path.Base(scope)
}

// pick returns the one value of the declarations that keep accepts. found is false when none is kept, and
// ambiguous is true when the kept declarations give more than one value.
func pick(decls []constDecl, keep func(constDecl) bool) (value string, found, ambiguous bool) {
	for _, d := range decls {
		if !keep(d) {
			continue
		}
		if !found {
			value, found = d.value, true
			continue
		}
		if d.value != value {
			return "", true, true
		}
	}
	return value, found, false
}

// resolve returns the value an identifier names when it is read from scope. A qualified name, such as
// crate::vision::parts::NAME, prefers the declaration in the module that the path names. A bare name that a `use`
// renames takes the renamed constant. Otherwise the scope's own declaration is used, then the tree's. Each step
// needs one value, so an ambiguous name is refused.
func (c *constTable) resolve(scope, ident string) (string, bool) {
	name, module := ident, ""
	if i := strings.LastIndexAny(ident, ".:"); i >= 0 {
		name = ident[i+1:]
		prefix := strings.TrimRight(ident[:i], ".:")
		if j := strings.LastIndexAny(prefix, ".:"); j >= 0 {
			module = prefix[j+1:]
		} else {
			module = prefix
		}
	} else if a, ok := c.aliases[scope][ident]; ok {
		name, module = a.name, a.module
	}
	decls := c.byName[name]
	if module != "" {
		if v, found, ambiguous := pick(decls, func(d constDecl) bool { return scopeModule(d.scope) == module }); ambiguous {
			return "", false
		} else if found {
			return v, true
		}
	}
	if v, found, ambiguous := pick(decls, func(d constDecl) bool { return d.scope == scope }); ambiguous {
		return "", false
	} else if found {
		return v, true
	}
	v, found, ambiguous := pick(decls, func(constDecl) bool { return true })
	return v, found && !ambiguous
}

// buildConstTable records every string constant in the sources, under the scope that declares it, and the
// `use ... as` renames of each Rust file.
func buildConstTable(goFiles, rustFiles []sourceFile) *constTable {
	c := &constTable{byName: make(map[string][]constDecl), aliases: make(map[string]map[string]useAlias)}
	for _, f := range goFiles {
		dir := path.Dir(f.rel)
		for _, at := range wordStarts(f.text, "const ") {
			if strings.HasPrefix(f.text[at:], "const (\n") {
				end := strings.Index(f.text[at:], "\n)")
				if end < 0 {
					continue
				}
				for _, m := range goConstItem.FindAllStringSubmatch(f.text[at:at+end], -1) {
					c.byName[m[1]] = append(c.byName[m[1]], constDecl{scope: dir, value: m[2]})
				}
				continue
			}
			if m := goConstLine.FindStringSubmatch(f.text[at:min(at+300, len(f.text))]); m != nil {
				c.byName[m[1]] = append(c.byName[m[1]], constDecl{scope: dir, value: m[2]})
			}
		}
	}
	for _, f := range rustFiles {
		for _, at := range wordStarts(f.text, "const ") {
			if m := rustConstLine.FindStringSubmatch(f.text[at:min(at+300, len(f.text))]); m != nil {
				c.byName[m[1]] = append(c.byName[m[1]], constDecl{scope: f.rel, value: m[2]})
			}
		}
		for _, at := range wordStarts(f.text, "use ") {
			end := strings.IndexByte(f.text[at:], ';')
			if end < 0 {
				continue
			}
			for _, m := range rustUseAlias.FindAllStringSubmatch(f.text[at:at+end], -1) {
				if c.aliases[f.rel] == nil {
					c.aliases[f.rel] = make(map[string]useAlias)
				}
				c.aliases[f.rel][m[3]] = useAlias{module: m[1], name: m[2]}
			}
		}
	}
	return c
}

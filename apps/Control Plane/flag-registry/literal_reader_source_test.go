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
	"strconv"
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
//   - Rust name helpers: a function that reads the environment by one of its own parameters, exactly, such
//     as truthy_env(name), or env_exact_true(key) through std::env::var_os. Each call that passes it a
//     name, a quoted literal or a constant, reads that variable exactly, and a call whose name the scan
//     cannot resolve fails the test. A function that passes its parameter on to such a helper is one too.
//     A function that reads its parameter both exactly and leniently is mixed, and the scan does not
//     follow its calls.
//   - Rust lists: the BFF gateway's constant arrays of switch names that its config reads by the exact
//     literal true (exactTrueListNames). Each member is read as true.
//
// A call resolves to the functions of its own file with its name, and to every function of that name in the
// tree only when the file has none, and only when they all read the same literal. A constant resolves through
// its module, a `use ... as` alias, the scope that declares it, or the tree, and each step needs one value.
//
// A name with no registry row has no typed read to disagree with, so it is not checked.

// literalReaderRoots are the Model Plane trees the scan reads, and the BFF gateway's source, which reads Model Plane's
// exact-true switches too. Rust is read only under each crate's and service's src directory, so fixtures in other
// directories stay out.
var literalReaderRoots = []string{
	"apps/Model Plane/go/services",
	"apps/Model Plane/rust/crates",
	"apps/Model Plane/rust/services",
	"apps/Frontend Plane/verevonv3/apps/gateway/src",
}

// literalReaderFloor is the fewest variables the scan may find. A scan that finds fewer has lost a shape.
const literalReaderFloor = 100

// literalReaderPins are switches the scan must find. Each one exercises a shape the scan recognises, so a
// shape that stops matching fails its pin instead of dropping out of the check without a sign.
var literalReaderPins = []string{
	"AGENTIC_SUBSCRIPTION_ENABLED",                  // Rust: from_env passes the reads through from_values and switch_value_is_on
	"ALLOW_DEV_AUTH_BYPASS",                         // BFF: dev_only_flag, a name helper over env_exact_true, which reads var_os
	"APPROVAL_SCOPE_GRANTS_ENABLED",                 // Rust: scope_grants_from wraps switch_from over a named constant
	"BROWSER_CONSENT_API",                           // BFF: a value helper, browser_consent_api_enabled, over an env read
	"BROWSER_DOWNLOADS",                             // BFF: a value helper, browser_downloads_enabled, over an env read
	"BROWSER_TAB_MENTIONS",                          // BFF: the exact-true list BROWSER_EXACT_TRUE_SWITCHES
	"CHAT_RESEARCH_RUNS",                            // BFF: env_exact_true("CHAT_RESEARCH_RUNS"), which reads var_os
	"EVENT_TRIGGERS_ENABLED",                        // Rust: switch_from over a named constant
	"EVENT_TRIGGER_FIRE_ENABLED",                    // Go: os.Getenv compared with "true"
	"EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK", // Rust: a name helper, env_flag, called with a literal name
	"INFERENCE_ORG_OVERLAY",                         // Rust: a wrapper over a named constant
	"MEMORY_GATE_CAPABILITY_WRITES",                 // Go: lookup over a constant, compared with "true"
	"MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY",            // Rust: a name helper, truthy_env, called with a named constant
	"PROMOTE_ON_USE_DURABLE",                        // Rust: an environment read compared with "true" in the same statement
	"PROMOTE_ON_USE_ENABLED",                        // Rust: the same, over a literal name
	"PROVIDER_READ_DOCUMENT_URL",                    // Rust: a wrapper compared with Some("true"), over a path-qualified constant
	"RESEARCH_RUNS_ENABLED",                         // Rust: switch_from over a named constant
	"SESSION_THREAD_RECEIPT_ENFORCE",                // Rust: a wrapper over switch_from, over a named constant
	"SPACE_REACTIONS_API",                           // BFF: the exact-true list BFF_EXACT_TRUE_SPACE_SWITCHES
	"TOOL_POLICY_RULE_AUDIT_ENABLED",                // Go: lookup over a constant, compared with "true"
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
	// rustQuotedName matches a whole argument that is a quoted environment variable name, and captures the name.
	rustQuotedName = regexp.MustCompile(`^"([A-Z][A-Z0-9_]*)"$`)
	// rustParamName matches a named parameter, `name: Type`, and captures the name.
	rustParamName = regexp.MustCompile(`^(?:mut\s+)?([a-z_][a-z0-9_]*)\s*:`)
	// rustSelfParam matches a receiver parameter: self, &self, &'a mut self or mut self.
	rustSelfParam = regexp.MustCompile(`^(?:&\s*(?:'\w+\s+)?)?(?:mut\s+)?self\b`)
	// rustListMember matches one quoted switch name in a constant array.
	rustListMember = regexp.MustCompile(`"([A-Z][A-Z0-9_]*)"`)
	// rustCompareAt matches a comparison with the literal true or 1, of the value itself or of Some or Ok of it.
	rustCompareAt = regexp.MustCompile(`^==\s*(?:Some\(\s*|Ok\(\s*)?"(true|1)"`)
	// rustCompareAfter matches such a comparison later in the same statement, before a brace or a semicolon.
	rustCompareAfter = regexp.MustCompile(`^[^;{}]{0,200}?==\s*(?:Some\(\s*|Ok\(\s*)?"(true|1)"`)
	// rustLetBinding matches the start of `let name = `, and of the path that may precede an environment read
	// on its right-hand side, at the end of the text before the read.
	rustLetBinding = regexp.MustCompile(`let\s+(?:mut\s+)?([a-z_][a-z0-9_]*)\s*(?::[^=;]*)?=\s*(?:[A-Za-z_]\w*::)*$`)
	// rustFnStart matches the start of a function item on its own line, which ends the scope of a local.
	rustFnStart = regexp.MustCompile(`(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s`)
	// rustInlineModHead matches the head of an inline module, `mod name {`, at the start of the text it is given.
	rustInlineModHead = regexp.MustCompile(`^mod\s+([a-z_][a-z0-9_]*)\s*\{`)
	// rustPathAttr matches a #[path = "..."] attribute, and captures the path it names.
	rustPathAttr = regexp.MustCompile(`^#\[path\s*=\s*"([^"]*)"\s*\]`)
	// rustModDecl matches a `mod NAME;` declaration, which loads a file, with any visibility.
	rustModDecl = regexp.MustCompile(`^(?:pub(?:\([^)]*\))?\s+)?mod\s+([a-z_][a-z0-9_]*)\s*;`)
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

// TestLiteralReaderScanIgnoresTestItems pins what the scan reads. A test module may hold strings that spell a read,
// such as "switch_from(std::env::var(X)...)", and a comment may name one. Neither is a reader, so the scan must not
// report them as unnamed. A #[cfg(test)] item is left out whole, and the reads beside it keep their line numbers.
func TestLiteralReaderScanIgnoresTestItems(t *testing.T) {
	const rel = "apps/Model Plane/rust/services/x/src/prod.rs"
	src := `pub const ENABLED_ENV: &str = "PROD_SWITCH_ENABLED";

// A comment that spells std::env::var(COMMENT_ONLY).ok().as_deref() == Some("true") is not a read.
pub fn prod() -> bool {
    switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spells_reads() {
        let cases = [
            "switch_from(std::env::var(X).ok().as_deref())",
            "std::env::var(X).ok().as_deref() == Some(\"1\")",
        ];
        assert_eq!(cases.len(), 2);
    }
}

#[cfg(test)]
fn one_line_helper() -> bool { std::env::var(ONE_LINE_TEST).ok().as_deref() == Some("true") }

pub fn after_tests() -> bool {
    switch_from(std::env::var("AFTER_TESTS_ENABLED").ok().as_deref())
}
`
	sites, unnamed := findLiteralReaders(nil, []sourceFile{{rel: rel, text: src}})
	if len(unnamed) != 0 {
		t.Errorf("a read inside a test item was taken for production: %+v", unnamed)
	}
	var got []string
	for _, s := range sites {
		got = append(got, s.name+" "+s.literal+" "+s.where)
	}
	slices.Sort(got)
	want := []string{
		"AFTER_TESTS_ENABLED true " + rel + ":26",
		"PROD_SWITCH_ENABLED true " + rel + ":5",
	}
	if !slices.Equal(got, want) {
		t.Errorf("sites = %v, want %v", got, want)
	}
}

// TestConstantPathsResolveThroughTheirModule pins how a path to a constant resolves: through the module the path
// names, and that module may be an inline `mod` block inside a file. Two constants with one name and two values
// therefore never clash, and a bare name that two modules declare is refused.
func TestConstantPathsResolveThroughTheirModule(t *testing.T) {
	files := productionSources([]sourceFile{
		{rel: "apps/Model Plane/rust/services/x/src/quarry.rs", text: `pub mod attribution {
    pub const ENABLED_ENV: &str = "MODEL_GATEWAY_QUARRY_ATTRIBUTION";
}

pub mod other {
    pub const ENABLED_ENV: &str = "OTHER_SWITCH_ENABLED";
}
`},
		{rel: "apps/Model Plane/rust/services/x/src/space/events.rs", text: `pub const EMIT_ENV: &str = "SPACE_EVENTS_EMIT";
`},
		{rel: "apps/Model Plane/rust/services/x/src/space/attention_outbox.rs", text: `pub const EMIT_ENV: &str = "SPACE_ATTENTION_EMIT";
`},
	})
	c := buildConstTable(nil, files)
	const scope = "apps/Model Plane/rust/services/x/src/other.rs"
	for _, tc := range []struct{ ident, want string }{
		{"crate::quarry::attribution::ENABLED_ENV", "MODEL_GATEWAY_QUARRY_ATTRIBUTION"},
		{"crate::quarry::other::ENABLED_ENV", "OTHER_SWITCH_ENABLED"},
		{"crate::space::events::EMIT_ENV", "SPACE_EVENTS_EMIT"},
		{"crate::space::attention_outbox::EMIT_ENV", "SPACE_ATTENTION_EMIT"},
	} {
		got, ok := c.resolve(scope, tc.ident)
		if !ok || got != tc.want {
			t.Errorf("%s resolves to %q (ok %v), want %q", tc.ident, got, ok, tc.want)
		}
	}
	if v, ok := c.resolve(scope, "EMIT_ENV"); ok {
		t.Errorf("a bare EMIT_ENV that two modules declare resolves to %q; it must be refused", v)
	}
}

// TestPathAttributeNamesTheModuleOfItsFile pins the module a #[path] attribute gives a file. Model-gateway's vision.rs
// declares `pub mod parts;` under `#[path = "vision_parts.rs"]`, so the file's constants are read as parts::NAME, and the
// file stem names no module for them: vision_parts::NAME does not resolve. The Rust side pins the same rule in
// a_read_through_the_declared_module_name_is_seen_and_the_file_stem_is_not. A #[path] inside an inline module resolves from
// that module's directory: in the non-mod-rs file a.rs, `mod inner { #[path = "x.rs"] pub mod y; }` loads a/inner/x.rs.
func TestPathAttributeNamesTheModuleOfItsFile(t *testing.T) {
	const (
		src    = "apps/Model Plane/rust/services/x/src/"
		reader = src + "reader.rs"
	)
	sources := []sourceFile{
		{rel: src + "vision.rs", text: "#[path = \"vision_parts.rs\"]\npub mod parts;\n"},
		{rel: src + "vision_parts.rs", text: "pub const ENABLED_VAR: &str = \"CHAT_VISION_AS_PARTS_ENABLED\";\n"},
		{rel: src + "other.rs", text: "pub const ENABLED_VAR: &str = \"OTHER_SWITCH_ENABLED\";\n"},
		{rel: src + "a.rs", text: "mod inner {\n    #[path = \"x.rs\"]\n    pub mod y;\n}\n"},
		{rel: src + "a/inner/x.rs", text: "pub const NESTED_ENV: &str = \"NESTED_ENABLED\";\n"},
		// The file a directory-relative resolution would name instead, so the read must reach the file through module y.
		{rel: src + "x.rs", text: "pub const NESTED_ENV: &str = \"WRONG_DIRECTORY_ENABLED\";\n"},
		{rel: reader, text: "pub fn on() -> bool {\n    switch_from(std::env::var(parts::ENABLED_VAR).ok().as_deref())\n}\n" +
			"pub fn stem() -> bool {\n    switch_from(std::env::var(vision_parts::ENABLED_VAR).ok().as_deref())\n}\n"},
	}
	c := buildConstTable(nil, productionSources(sources))
	for _, tc := range []struct{ ident, want string }{
		{"crate::vision::parts::ENABLED_VAR", "CHAT_VISION_AS_PARTS_ENABLED"},
		{"parts::ENABLED_VAR", "CHAT_VISION_AS_PARTS_ENABLED"},
		{"crate::a::inner::y::NESTED_ENV", "NESTED_ENABLED"},
	} {
		if got, ok := c.resolve(reader, tc.ident); !ok || got != tc.want {
			t.Errorf("%s resolves to %q (ok %v), want %q", tc.ident, got, ok, tc.want)
		}
	}
	if got, ok := c.resolve(reader, "vision_parts::ENABLED_VAR"); ok {
		t.Errorf("vision_parts::ENABLED_VAR resolves to %q; the file stem names no module of a #[path] file", got)
	}

	// The scan sees the same: the parts read is a site, and the stem read is one the scan cannot name.
	sites, unnamed := findLiteralReaders(nil, sources)
	var got []string
	for _, s := range sites {
		got = append(got, s.name+" "+s.literal+" "+s.where)
	}
	if want := []string{"CHAT_VISION_AS_PARTS_ENABLED true " + reader + ":2"}; !slices.Equal(got, want) {
		t.Errorf("sites = %v, want %v", got, want)
	}
	if len(unnamed) != 1 || unnamed[0].ident != "vision_parts::ENABLED_VAR" {
		t.Errorf("unnamed = %+v, want the one vision_parts::ENABLED_VAR read", unnamed)
	}
}

// TestProductionRustBlanksEachTestItemShape pins the shapes productionRust blanks: a #[cfg(test)] that shares its line
// with a multi-line item, one on its own line in front of a one-line item and in front of a multi-line item, and a comment
// line. It also pins the heads a test attribute can have: a stacked attribute with comments between, an enum variant, a
// match arm, a one-line item with a trailing comment, a multi-line attribute, a bracketed item, and a head the scan cannot
// close. It pins a blank line inside an indented module, and the raw strings: a fixture inside a test module is blank through
// the module's brace, a test attribute inside a production raw string is text, and a string of the letter r opens nothing.
// Every line stays in place, and only the lines of the test items and the comments are blank. Each shape is followed by a
// production line, which must survive.
func TestProductionRustBlanksEachTestItemShape(t *testing.T) {
	src := strings.Join([]string{
		"fn keep_a() {}",
		"#[cfg(test)] mod tests {",
		"    fn t() { std::env::var(X); }",
		"}",
		"// a comment that names var(COMMENT)",
		"fn keep_b() {}",
		"#[cfg(test)]",
		"use crate::{a, b};",
		"fn keep_c() {}",
		"#[cfg(test)]",
		"mod more {",
		"    fn t() {}",
		"}",
		"fn keep_d() {}",
		"#[cfg(test)]",
		"// a comment between the attributes",
		"/// a doc comment between the attributes",
		"#[path = \"t.rs\"]",
		"mod t;",
		"pub const KEEP_A_ENV: &str = \"KEEP_A_ENABLED\";",
		"enum Credentials {",
		"    Service,",
		"    #[cfg(test)]",
		"    Static,",
		"    Other,",
		"}",
		"fn keep_e() {}",
		"impl Gate {",
		"    fn arm(&self, v: u8) -> Result<u8, u8> {",
		"        match v {",
		"            1 => Ok(1),",
		"            #[cfg(test)]",
		"            2 => Ok(2),",
		"            _ => Ok(3),",
		"        }",
		"    }",
		"}",
		"fn keep_f() {}",
		"mod outer {",
		"    #[cfg(test)]",
		"    mod tests_b {",
		"        fn a() {}",
		"",
		"        fn b() {}",
		"    }",
		"    fn keep_g() {}",
		"}",
		"enum Last {",
		"    Service,",
		"    #[cfg(test)]",
		"    Static",
		"}",
		"fn keep_h() {}",
		"#[cfg(test)]",
		"use crate::a; // the test's import",
		"pub fn keep_i() -> bool {",
		"    true",
		"}",
		"#[cfg(test)]",
		"#[cfg_attr(",
		"    feature = \"x\",",
		"    allow(dead_code)",
		")]",
		"mod tests_c;",
		"fn keep_j() {}",
		"#[cfg(test)]",
		"const CASES: &[&str] = &[",
		"    \"a\",",
		"];",
		"pub fn keep_k() {}",
		"#[cfg(test)]",
		"const LONG_ENV: &str =",
		"    \"LONG_ENABLED\";",
		"pub fn keep_l() {}",
		"#[cfg(test)]",
		"mod fixtures {",
		"const FIXTURE: &str = r#\"",
		"fn fixture_item() {}",
		"}",
		")",
		"\"#;",
		"}",
		"pub fn keep_m() {}",
		"pub const SPEC: &str = r#\"",
		"#[cfg(test)]",
		"mod spec {",
		"}",
		"\"#;",
		"pub fn keep_n() -> bool {",
		"    std::env::var(\"KEEP_N_ENABLED\").is_ok()",
		"}",
		"pub const MODE: &str = \"r\";",
		"#[cfg(test)]",
		"mod modes {",
		"}",
		"pub fn keep_o() {}",
		"",
	}, "\n")
	want := []string{
		"fn keep_a() {}", // line 1
		"",               // line 2: the attribute and the test module
		"",               // line 3
		"",               // line 4: the closing brace
		"",               // line 5: the comment
		"fn keep_b() {}", // line 6
		"",               // line 7: the attribute
		"",               // line 8: the use, a one-line item
		"fn keep_c() {}", // line 9
		"",               // line 10: the attribute
		"",               // line 11: the multi-line module
		"",               // line 12
		"",               // line 13: the closing brace
		"fn keep_d() {}", // line 14
		// The stacked attributes, with a comment and a doc comment between them: the attributes and the module are blank.
		"",
		"",
		"",
		"",
		"",
		"pub const KEEP_A_ENV: &str = \"KEEP_A_ENABLED\";",
		// The enum variant: the attribute and the variant are blank, and the variants around it survive.
		"enum Credentials {",
		"    Service,",
		"",
		"",
		"    Other,",
		"}",
		"fn keep_e() {}",
		// The match arm: the attribute and the arm are blank, and the arms around it survive.
		"impl Gate {",
		"    fn arm(&self, v: u8) -> Result<u8, u8> {",
		"        match v {",
		"            1 => Ok(1),",
		"",
		"",
		"            _ => Ok(3),",
		"        }",
		"    }",
		"}",
		"fn keep_f() {}",
		// A blank line inside the indented test module does not end the module.
		"mod outer {",
		"",
		"",
		"",
		"",
		"",
		"",
		"    fn keep_g() {}",
		"}",
		// The head that the scan cannot close before the enclosing block ends: only the head is blank.
		"enum Last {",
		"    Service,",
		"",
		"",
		"}",
		"fn keep_h() {}",
		// A one-line item with a trailing comment is one line, so the function after it survives.
		"",
		"",
		"pub fn keep_i() -> bool {",
		"    true",
		"}",
		// A multi-line attribute runs to its closing bracket, and the item after it is one line.
		"",
		"",
		"",
		"",
		"",
		"",
		"fn keep_j() {}",
		// A bracketed item ends at its closing bracket, so the function after it survives.
		"",
		"",
		"",
		"",
		"pub fn keep_k() {}",
		// A value continued on the next line has no closer, so the scan stops at the next item: only the head is blank.
		"",
		"",
		"    \"LONG_ENABLED\";",
		"pub fn keep_l() {}",
		// A module whose fixture holds column-0 items, a closing brace and brackets is blank through its closing brace.
		"",
		"",
		"",
		"",
		"",
		"",
		"",
		"",
		"pub fn keep_m() {}",
		// A production raw string holds a test attribute at column 0: it is text, so every line of it is kept.
		"pub const SPEC: &str = r#\"",
		"#[cfg(test)]",
		"mod spec {",
		"}",
		"\"#;",
		"pub fn keep_n() -> bool {",
		"    std::env::var(\"KEEP_N_ENABLED\").is_ok()",
		"}",
		// A string that is the single letter r does not open a raw string, so the module after it is an item.
		"pub const MODE: &str = \"r\";",
		"",
		"",
		"",
		"pub fn keep_o() {}",
		"",
	}
	if got := strings.Split(productionRust(src), "\n"); !slices.Equal(got, want) {
		t.Errorf("productionRust kept lines %q, want %q", got, want)
	}
}

// TestRawStringOpenFollowsLiterals pins how a line leaves a raw string open. A raw string opened by r" or br#" stays open until
// its closing quote and hashes. A line comment, a normal string, a character literal and an identifier ending in r open none.
func TestRawStringOpenFollowsLiterals(t *testing.T) {
	for _, tc := range []struct {
		line string
		open int
		want int
	}{
		{`const S: &str = r#"`, -1, 1},
		{`"#;`, 1, -1},
		{`let s = r##"a"#b"##;`, -1, -1},
		{`let s = br"x`, -1, 0},
		{`let mode = "r";`, -1, -1},
		{`let c = '"'; let s = r#"`, -1, 1},
		{`let c = 'a'; let t = 'b;`, -1, -1},
		{`// a comment with r#" in it`, -1, -1},
		{`let bar = foo_r"x";`, -1, -1},
		{`    still inside the raw string // r#"`, 1, 1},
	} {
		if got := rawStringOpen(tc.line, tc.open); got != tc.want {
			t.Errorf("rawStringOpen(%q, %d) = %d, want %d", tc.line, tc.open, got, tc.want)
		}
	}
}

// TestProductionRustBlanksOnlyInsideItsAttributesBlock reads every Rust source of the Model Plane tree and checks each test
// item productionRust blanks. No line of an item that is not blank, a comment or text inside a raw string may be indented
// less than its attribute, so the blanking never reaches out of the block the attribute sits in. The walk must find the
// tree's test items, so an empty walk cannot pass.
func TestProductionRustBlanksOnlyInsideItsAttributesBlock(t *testing.T) {
	_, rustFiles := readLiteralReaderSources(t)
	items := 0
	for _, f := range rustFiles {
		lines := strings.Split(f.text, "\n")
		for _, s := range testItemSpans(lines) {
			items++
			raw := -1
			for k := s.first + 1; k <= s.last; k++ {
				inRaw := raw >= 0
				raw = rawStringOpen(lines[k], raw)
				body := strings.TrimLeft(lines[k], " \t")
				if inRaw || body == "" || strings.HasPrefix(body, "//") {
					continue
				}
				if indentOf(lines[k]) < indentOf(lines[s.first]) {
					t.Errorf("%s:%d: the test item at line %d blanks %q, which is indented less than its attribute", f.rel, k+1, s.first+1, lines[k])
				}
			}
		}
	}
	if items < 100 {
		t.Fatalf("the tree holds %d test items, want at least 100; the walk may have stopped finding them", items)
	}
}

// TestRustNameHelpersAreFollowedToTheirCalls pins the name helpers. A function that reads the environment by one of its
// own parameters, exactly, is followed to each call that names the variable: by a quoted literal, by a constant, or by a
// parameter that the caller passes on. A call whose name the scan cannot resolve is unnamed, and a local that names a read
// is unnamed too. A function that reads its parameter leniently, or both leniently and exactly, is not followed.
func TestRustNameHelpersAreFollowedToTheirCalls(t *testing.T) {
	const src = "apps/Model Plane/rust/services/x/src/"
	helpers := sourceFile{rel: src + "helpers.rs", text: `pub fn env_flag(name: &str) -> bool {
    std::env::var(name).ok().as_deref() == Some("true")
}

pub fn forward(name: &str) -> bool {
    env_flag(name)
}

pub fn lenient(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub fn mixed(name: &str) -> bool {
    env_flag(name) || std::env::var(name).is_ok()
}
`}
	user := sourceFile{rel: src + "user.rs", text: `const CONST_ENV: &str = "CONST_ENABLED";

pub fn literal() -> bool {
    env_flag("LITERAL_ENABLED")
}

pub fn constant() -> bool {
    env_flag(CONST_ENV)
}

pub fn passed_on() -> bool {
    forward("FORWARDED_ENABLED")
}

pub fn lenient_only() -> bool {
    lenient("LENIENT_ENABLED").is_some()
}

pub fn mixed_only() -> bool {
    mixed("MIXED_ENABLED")
}

pub fn computed() -> bool {
    env_flag(format!("X_{}", 1).as_str())
}

pub fn local() -> bool {
    let key = pick();
    std::env::var(key).ok().as_deref() == Some("true")
}
`}
	sites, unnamed := findLiteralReaders(nil, []sourceFile{helpers, user})
	var got []string
	for _, s := range sites {
		got = append(got, s.name+" "+s.literal)
	}
	slices.Sort(got)
	if want := []string{"CONST_ENABLED true", "FORWARDED_ENABLED true", "LITERAL_ENABLED true"}; !slices.Equal(got, want) {
		t.Errorf("sites = %q, want %q", got, want)
	}
	var idents []string
	for _, u := range unnamed {
		idents = append(idents, u.ident)
	}
	slices.Sort(idents)
	if want := []string{`format!("X_{}", 1).as_str()`, "key"}; !slices.Equal(idents, want) {
		t.Errorf("unnamed = %q, want %q", idents, want)
	}
}

// TestClosureArgumentsAreNotPassedToTheirReader pins the closure readers. A read inside a closure that is an argument, such
// as from_lookup(|name| std::env::var(name).ok()), is the closure's value and not the value the reader takes: the names the
// closure reads come from the callee. So the scan reads neither a site nor an unnamed read from it.
func TestClosureArgumentsAreNotPassedToTheirReader(t *testing.T) {
	const rel = "apps/Model Plane/rust/services/x/src/lookup.rs"
	src := `pub fn switch_on(lookup: impl Fn(&str) -> Option<String>) -> bool {
    lookup("NEVER_READ_ENABLED").as_deref() == Some("true")
}

pub fn from_env() -> bool {
    switch_on(|name| std::env::var(name).ok())
}
`
	sites, unnamed := findLiteralReaders(nil, []sourceFile{{rel: rel, text: src}})
	if len(sites) != 0 {
		t.Errorf("a closure argument was read as a site: %+v", sites)
	}
	if len(unnamed) != 0 {
		t.Errorf("a closure argument was reported as unnamed: %+v", unnamed)
	}
}

// TestExactTrueListMembersAreReadAsTrue pins the gateway's arrays. Each member of a declared array of exact-true switch names
// is read as the literal true, at the line that holds it.
func TestExactTrueListMembersAreReadAsTrue(t *testing.T) {
	const rel = "apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs"
	src := "pub(crate) const MODEL_PLANE_EXACT_TRUE_SWITCHES: [&str; 2] = [\n    \"ONE_ENABLED\",\n    \"TWO_ENABLED\",\n];\n"
	sites, unnamed := findLiteralReaders(nil, []sourceFile{{rel: rel, text: src}})
	var got []string
	for _, s := range sites {
		got = append(got, s.name+" "+s.literal+" "+s.where)
	}
	slices.Sort(got)
	want := []string{"ONE_ENABLED true " + rel + ":2", "TWO_ENABLED true " + rel + ":3"}
	if !slices.Equal(got, want) {
		t.Errorf("sites = %q, want %q", got, want)
	}
	if len(unnamed) != 0 {
		t.Errorf("unnamed = %+v, want none", unnamed)
	}
}

// TestVarOsEnvironmentReadsAreRecognised pins the reads through std::env::var_os, which the gateway's exact-true helper
// takes the variable with. A name helper that reads its parameter exactly through var_os is followed to each call that
// names the variable, so env_exact_true("NAME") and dev_only_flag("NAME") are read. A var_os read that only tests for
// presence is lenient, and names no switch.
func TestVarOsEnvironmentReadsAreRecognised(t *testing.T) {
	const rel = "apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs"
	src := `fn env_exact_true(key: &str) -> bool {
    std::env::var_os(key).is_some_and(|value| value.to_str() == Some("true"))
}

fn dev_only_flag(key: &str) -> bool {
    env_exact_true(key)
}

pub(crate) fn build() -> bool {
    dev_only_flag("VAR_OS_DEV_ENABLED") || env_exact_true("VAR_OS_EXACT_ENABLED")
}

pub(crate) fn present() -> bool {
    std::env::var_os("VAR_OS_PRESENT_ENABLED").is_some()
}
`
	sites, unnamed := findLiteralReaders(nil, []sourceFile{{rel: rel, text: src}})
	var got []string
	for _, s := range sites {
		got = append(got, s.name+" "+s.literal+" "+s.where)
	}
	slices.Sort(got)
	if want := []string{"VAR_OS_DEV_ENABLED true " + rel + ":10", "VAR_OS_EXACT_ENABLED true " + rel + ":10"}; !slices.Equal(got, want) {
		t.Errorf("sites = %q, want %q", got, want)
	}
	if len(unnamed) != 0 {
		t.Errorf("unnamed = %+v, want none", unnamed)
	}
}

// TestBFFExactTrueListsAreReadInFull reads the gateway's exact-true arrays from the tree. Each is declared once, in the
// gateway's config, and holds as many names as its type declares, so a name added without its count fails here.
func TestBFFExactTrueListsAreReadInFull(t *testing.T) {
	_, rustFiles := readLiteralReaderSources(t)
	const config = "apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs"
	declared := make(map[string]int)
	members := 0
	for _, l := range exactTrueLists(rustFiles) {
		declared[l.name]++
		if l.file != config {
			t.Errorf("%s is declared in %s, want %s", l.name, l.file, config)
		}
		if len(l.members) != l.declared {
			t.Errorf("%s declares %d names but holds %d", l.name, l.declared, len(l.members))
		}
		members += len(l.members)
	}
	for _, name := range exactTrueListNames {
		if declared[name] != 1 {
			t.Errorf("%s is declared %d times in the sources, want once", name, declared[name])
		}
	}
	if members < 20 {
		t.Errorf("the exact-true arrays hold %d names, want at least 20", members)
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
	rustFiles = productionSources(rustFiles)
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

	// Rust: the value readers are switch_from and every function that reaches it, or compares with the literal.
	// Each environment read is classified once. An exact read names its variable: a quoted literal or a constant
	// is emitted here, and a parameter is a use of a name helper, whose calls are followed below.
	var fns []rustFn
	fileFns := make(map[string][]int)
	texts := make(map[string]string, len(rustFiles))
	for _, f := range rustFiles {
		texts[f.rel] = f.text
		for _, fn := range rustFunctions(f.rel, f.text) {
			fileFns[f.rel] = append(fileFns[f.rel], len(fns))
			fns = append(fns, fn)
		}
	}
	readers := rustExactReaders(fns)
	reads := rustReadsOf(rustFiles, fns, fileFns, readers)
	names := newRustNames(fns)
	uses := rustNameUses(fns, reads, names)
	for _, r := range reads {
		if len(r.exact) == 0 || (r.fn >= 0 && paramIndex(fns[r.fn], r.arg) >= 0) {
			continue
		}
		for _, lit := range r.exact {
			emit(r.file, r.file, texts[r.file], r.at, r.lit, r.arg, lit)
		}
	}
	// A call to a name helper that reads its parameter exactly names the variable it is passed. The argument is
	// resolved from the scope of the call, so a constant of the caller's file or module is found.
	for _, f := range rustFiles {
		calls := identCalls(f.text)
		for _, name := range slices.Sorted(maps.Keys(calls)) {
			pool := names.pool(f.rel, name)
			if len(pool) == 0 {
				continue
			}
			for _, c := range calls[name] {
				if isFnDefinition(f.text, c.start) {
					continue
				}
				args, ok := callArgs(f.text, c.open)
				if !ok {
					continue
				}
				enclosing := innermostFn(fns, fileFns[f.rel], c.start)
				for j, arg := range splitArgs(args) {
					u := poolUse(pool, uses, j)
					if !u.exact || u.lenient {
						continue
					}
					// A parameter passed on is a use of the caller, which its own call sites name.
					if enclosing >= 0 && paramIndex(fns[enclosing], normalizeName(arg)) >= 0 {
						continue
					}
					lit, ident := rustArgName(arg)
					emit(f.rel, f.rel, f.text, c.start, lit, ident, u.lit)
				}
			}
		}
	}
	// The BFF gateway's arrays of exact-true switch names: each member is read as the literal true.
	for _, l := range exactTrueLists(rustFiles) {
		for _, m := range l.members {
			sites = append(sites, literalReaderSite{name: m.name, literal: "true", where: fmt.Sprintf("%s:%d", l.file, m.line)})
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

// rustEnvReads returns each env::var, env::var_os or get read in text, in order. The variable is the call's argument: a quoted
// literal name, or else the argument as written, with surrounding space and a leading & removed. A function item
// named var, var_os or get is a declaration, not a read.
func rustEnvReads(text string) []envRead {
	var out []envRead
	for _, word := range []string{"var(", "var_os(", "get("} {
		for _, at := range wordStarts(text, word) {
			if isFnDefinition(text, at) {
				continue
			}
			open := at + len(word) - 1
			args, ok := callArgs(text, open)
			if !ok {
				continue
			}
			r := envRead{at: at, end: open + len(args) + 2}
			if m := rustQuotedName.FindStringSubmatch(strings.TrimSpace(args)); m != nil {
				r.lit = m[1]
			} else {
				r.ident = normalizeName(args)
			}
			out = append(out, r)
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

// rustFn is one function item that has a body: the file that declares it, its name, its parameter names in order
// (without a receiver, and empty for a pattern), the text between its braces, and where those braces sit in the
// file's text.
type rustFn struct {
	file       string
	name       string
	body       string
	params     []string
	start, end int
}

// rustFunctions returns the functions with a body in one file.
func rustFunctions(file, text string) []rustFn {
	var out []rustFn
	for _, at := range wordStarts(text, "fn ") {
		m := rustFnName.FindStringSubmatchIndex(text[at:min(at+200, len(text))])
		if m == nil {
			continue
		}
		open := rustParamsOpen(text, at+m[1])
		if open < 0 {
			continue
		}
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
		out = append(out, rustFn{
			file:   file,
			name:   text[at+m[2] : at+m[3]],
			body:   text[start+1 : end],
			params: rustParamNames(text[open+1 : closeParen]),
			start:  start,
			end:    end,
		})
	}
	return out
}

// rustParamsOpen returns the index of the parenthesis that opens a function's parameters, after its name and any
// generic parameters, or -1 when there is none.
func rustParamsOpen(text string, from int) int {
	i := from
	for i < len(text) && (text[i] == ' ' || text[i] == '\t') {
		i++
	}
	if i < len(text) && text[i] == '<' {
		depth := 0
		for ; i < len(text); i++ {
			if text[i] == '<' {
				depth++
			} else if text[i] == '>' {
				depth--
			}
			if depth == 0 {
				i++
				break
			}
		}
	}
	i = min(i, len(text))
	k := strings.IndexByte(text[i:], '(')
	if k < 0 {
		return -1
	}
	return i + k
}

// rustParamNames returns the parameter names of a parameter list, without its receiver. A parameter that is a pattern
// has the empty name, so the position of each parameter still counts.
func rustParamNames(list string) []string {
	var names []string
	for _, p := range splitParams(list) {
		p = strings.TrimSpace(p)
		if p == "" || rustSelfParam.MatchString(p) {
			continue
		}
		name := ""
		if m := rustParamName.FindStringSubmatch(p); m != nil {
			name = m[1]
		}
		names = append(names, name)
	}
	return names
}

// splitArgs splits the text between a call's parentheses into its arguments. A comma inside a nested bracket does not
// split.
func splitArgs(text string) []string {
	return splitTopLevel(text, false)
}

// splitParams splits a parameter list into its parameters. A comma inside a generic type, such as HashMap<K, V>, does
// not split either.
func splitParams(text string) []string {
	return splitTopLevel(text, true)
}

// splitTopLevel splits text at each comma that is outside every bracket, and outside the angle brackets too when angles
// is set. A trailing comma adds no empty item.
func splitTopLevel(text string, angles bool) []string {
	var out []string
	depth, from := 0, 0
	for i := 0; i < len(text); i++ {
		switch text[i] {
		case '(', '[', '{':
			depth++
		case ')', ']', '}':
			depth--
		case '<':
			if angles {
				depth++
			}
		case '>':
			if angles && (i == 0 || text[i-1] != '-') {
				depth--
			}
		case ',':
			if depth == 0 {
				out = append(out, text[from:i])
				from = i + 1
			}
		}
	}
	if strings.TrimSpace(text[from:]) != "" {
		out = append(out, text[from:])
	}
	return out
}

// normalizeName returns an argument as a name: surrounding space and a leading & removed.
func normalizeName(arg string) string {
	return strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(arg), "&"))
}

// rustArgName returns the variable an environment read or a name helper's argument names: its quoted literal, or else
// the argument as a name for the constant lookup.
func rustArgName(arg string) (lit, ident string) {
	if m := rustQuotedName.FindStringSubmatch(strings.TrimSpace(arg)); m != nil {
		return m[1], ""
	}
	return "", normalizeName(arg)
}

// paramIndex returns the position of the parameter named name in fn, or -1. An empty name matches nothing.
func paramIndex(fn rustFn, name string) int {
	if name == "" {
		return -1
	}
	for k, p := range fn.params {
		if p == name {
			return k
		}
	}
	return -1
}

// innermostFn returns the index, among the functions idxs of one file, of the innermost function whose body holds the
// byte at, or -1 when none does.
func innermostFn(fns []rustFn, idxs []int, at int) int {
	best := -1
	for _, i := range idxs {
		if fns[i].start < at && at < fns[i].end && (best < 0 || fns[i].start > fns[best].start) {
			best = i
		}
	}
	return best
}

// isFnDefinition reports whether the name that starts at at is declared by a fn keyword just before it, so that the
// text there is a function item and not a call.
func isFnDefinition(text string, at int) bool {
	before := strings.TrimRight(text[max(0, at-16):at], " \t\r\n")
	if !strings.HasSuffix(before, "fn") {
		return false
	}
	return len(before) == 2 || !isIdentByte(before[len(before)-3])
}

// rustRead is one environment read in a Rust file: where it starts, its variable (a quoted literal name in lit, or the
// argument as written in arg, the other being empty), the function that holds it or -1, and the literals its value is
// compared with exactly. A lenient read has no literal.
type rustRead struct {
	file  string
	at    int
	lit   string
	arg   string
	fn    int
	exact []string
}

// rustReadsOf returns every environment read in the Rust sources, file by file.
func rustReadsOf(rustFiles []sourceFile, fns []rustFn, fileFns map[string][]int, readers *rustReaders) []rustRead {
	var out []rustRead
	for _, f := range rustFiles {
		reads := rustEnvReads(f.text)
		exact := rustExactLiterals(f.rel, f.text, reads, readers)
		for _, r := range reads {
			out = append(out, rustRead{
				file:  f.rel,
				at:    r.at,
				lit:   r.lit,
				arg:   r.ident,
				fn:    innermostFn(fns, fileFns[f.rel], r.at),
				exact: exact[r.at],
			})
		}
	}
	return out
}

// rustExactLiterals returns, keyed by the byte each environment read starts at, the literals that read's value is
// compared with exactly. A read is exact when its statement compares it with a literal, when it is an argument of a
// value reader, or when it is bound to a local that a value reader takes later in the same function. A read compared
// inside its own argument is compared, not passed on.
func rustExactLiterals(file, text string, reads []envRead, readers *rustReaders) map[int][]string {
	exact := make(map[int][]string)
	add := func(at int, lit string) {
		if !slices.Contains(exact[at], lit) {
			exact[at] = append(exact[at], lit)
		}
	}
	// An environment read compared with the literal in the same statement, such as a map closure.
	for _, r := range reads {
		if cm := rustCompareAfter.FindStringSubmatch(text[r.end:min(r.end+200, len(text))]); cm != nil {
			add(r.at, cm[1])
		}
	}
	// An environment read bound to a local, which a value reader later in the same function takes as an argument.
	for _, r := range reads {
		m := rustLetBinding.FindStringSubmatch(text[max(0, r.at-160):r.at])
		if m == nil {
			continue
		}
		window := text[r.end:min(r.end+600, len(text))]
		if loc := rustFnStart.FindStringIndex(window); loc != nil {
			window = window[:loc[0]]
		}
		calls := identCalls(window)
		for _, name := range slices.Sorted(maps.Keys(calls)) {
			lit, ok := readers.resolve(file, name)
			if !ok {
				continue
			}
			for _, c := range calls[name] {
				if args, ok := callArgs(window, c.open); ok && hasWord(args, m[1]) {
					add(r.at, lit)
				}
			}
		}
	}
	// An environment read passed to a value reader.
	calls := identCalls(text)
	for _, name := range slices.Sorted(maps.Keys(calls)) {
		lit, ok := readers.resolve(file, name)
		if !ok {
			continue
		}
		for _, c := range calls[name] {
			args, ok := callArgs(text, c.open)
			if !ok {
				continue
			}
			for _, r := range rustEnvReads(args) {
				// A read compared inside its own argument is not passed to the reader. The compare pass takes it.
				if rest := argRest(args[r.end:]); strings.Contains(rest, "==") || strings.Contains(rest, "!=") {
					continue
				}
				// A read inside a closure that is the argument, such as from_lookup(|name| env::var(name).ok()), is
				// not the value the reader takes: the closure is, and the names it reads come from the callee.
				if insideClosure(args, r.at) {
					continue
				}
				add(c.open+1+r.at, lit)
			}
		}
	}
	return exact
}

// insideClosure reports whether the byte at of a call's argument text lies in a closure: the argument that holds it
// opens a closure with a `|`, and the rest of that argument is the closure's body.
func insideClosure(args string, at int) bool {
	start, depth := 0, 0
	for i := 0; i < at; i++ {
		switch args[i] {
		case '(', '[', '{':
			depth++
		case ')', ']', '}':
			depth--
		case ',':
			if depth == 0 {
				start = i + 1
			}
		}
	}
	return strings.Contains(strings.ReplaceAll(args[start:at], "||", ""), "|")
}

// rustNames indexes the Rust functions by name: those of each file, and those of the whole tree.
type rustNames struct {
	byFile map[string]map[string][]int
	byName map[string][]int
}

// newRustNames indexes fns.
func newRustNames(fns []rustFn) *rustNames {
	n := &rustNames{byFile: make(map[string]map[string][]int), byName: make(map[string][]int)}
	for i, fn := range fns {
		if n.byFile[fn.file] == nil {
			n.byFile[fn.file] = make(map[string][]int)
		}
		n.byFile[fn.file][fn.name] = append(n.byFile[fn.file][fn.name], i)
		n.byName[fn.name] = append(n.byName[fn.name], i)
	}
	return n
}

// pool returns the functions that a call to name from file may reach: the functions of that file with the name, and
// otherwise every function of the name in the tree. It is the rule the value readers follow too.
func (n *rustNames) pool(file, name string) []int {
	if local := n.byFile[file][name]; len(local) > 0 {
		return local
	}
	return n.byName[name]
}

// rustUse is what a function does with one of its parameters, when an environment read takes that parameter as the
// variable's name. It reads the variable exactly with the literal lit, reads it leniently, or does both. A parameter that
// does both is mixed, and the scan does not follow its calls: its exact names are not all of its names.
type rustUse struct {
	exact, lenient bool
	lit            string
}

// mergeUse adds the use src to dst. Two exact uses with different literals make the parameter mixed.
func mergeUse(dst *rustUse, src rustUse) {
	if src.lenient {
		dst.lenient = true
	}
	if !src.exact {
		return
	}
	if dst.exact && dst.lit != src.lit {
		dst.lenient = true
		return
	}
	dst.exact, dst.lit = true, src.lit
}

// poolUse returns the use that the functions of a pool make of their j-th parameter.
func poolUse(pool []int, uses [][]rustUse, j int) rustUse {
	var out rustUse
	for _, i := range pool {
		if j < len(uses[i]) {
			mergeUse(&out, uses[i][j])
		}
	}
	return out
}

// rustNameUses returns, for each function and each parameter position, the use the function makes of that parameter as
// a variable name. A direct read of the parameter is exact when the read is exact, and lenient otherwise. A call that
// passes the parameter on takes the use that the callee makes of the argument, and the uses are carried through a chain
// of such calls to a fixed point.
func rustNameUses(fns []rustFn, reads []rustRead, names *rustNames) [][]rustUse {
	direct := make([][]rustUse, len(fns))
	for i, fn := range fns {
		direct[i] = make([]rustUse, len(fn.params))
	}
	for _, r := range reads {
		if r.fn < 0 {
			continue
		}
		k := paramIndex(fns[r.fn], r.arg)
		if k < 0 {
			continue
		}
		if len(r.exact) == 0 {
			mergeUse(&direct[r.fn][k], rustUse{lenient: true})
		}
		for _, lit := range r.exact {
			mergeUse(&direct[r.fn][k], rustUse{exact: true, lit: lit})
		}
	}
	// A forward is a call that passes one of the function's parameters as its j-th argument.
	type forward struct {
		param, arg   int
		file, callee string
	}
	forwards := make([][]forward, len(fns))
	for i, fn := range fns {
		if len(fn.params) == 0 {
			continue
		}
		calls := identCalls(fn.body)
		for _, name := range slices.Sorted(maps.Keys(calls)) {
			for _, c := range calls[name] {
				if isFnDefinition(fn.body, c.start) {
					continue
				}
				args, ok := callArgs(fn.body, c.open)
				if !ok {
					continue
				}
				for j, arg := range splitArgs(args) {
					if k := paramIndex(fn, normalizeName(arg)); k >= 0 {
						forwards[i] = append(forwards[i], forward{param: k, arg: j, file: fn.file, callee: name})
					}
				}
			}
		}
	}
	uses := make([][]rustUse, len(fns))
	for i := range fns {
		uses[i] = slices.Clone(direct[i])
	}
	for changed := true; changed; {
		changed = false
		for i := range fns {
			for k := range fns[i].params {
				next := direct[i][k]
				for _, f := range forwards[i] {
					if f.param == k {
						mergeUse(&next, poolUse(names.pool(f.file, f.callee), uses, f.arg))
					}
				}
				if next != uses[i][k] {
					uses[i][k] = next
					changed = true
				}
			}
		}
	}
	return uses
}

// exactTrueListNames are the BFF gateway's constant arrays of switch names that the gateway reads by the exact literal
// true. Its config reads each member through a helper that takes the name, so the scan reads the arrays themselves.
var exactTrueListNames = []string{
	"BFF_EXACT_TRUE_SPACE_SWITCHES",
	"BROWSER_EXACT_TRUE_SWITCHES",
	"MODEL_PLANE_EXACT_TRUE_SWITCHES",
}

// exactTrueList is one such array as it is declared: its name, its file, the length its type declares, and its members.
type exactTrueList struct {
	name, file string
	declared   int
	members    []listMember
}

// listMember is one switch name of an array, with the line that holds it.
type listMember struct {
	name string
	line int
}

// exactTrueLists returns each declaration of the arrays named in exactTrueListNames that the sources hold.
func exactTrueLists(rustFiles []sourceFile) []exactTrueList {
	var out []exactTrueList
	for _, name := range exactTrueListNames {
		decl := regexp.MustCompile(`const\s+` + name + `\s*:\s*\[&(?:'static\s+)?str;\s*(\d+)\]\s*=\s*\[`)
		for _, f := range rustFiles {
			loc := decl.FindStringSubmatchIndex(f.text)
			if loc == nil {
				continue
			}
			end := strings.Index(f.text[loc[1]:], "];")
			if end < 0 {
				continue
			}
			declared, _ := strconv.Atoi(f.text[loc[2]:loc[3]])
			list := exactTrueList{name: name, file: f.rel, declared: declared}
			body := loc[1]
			for _, m := range rustListMember.FindAllStringSubmatchIndex(f.text[body:body+end], -1) {
				at := body + m[2]
				list.members = append(list.members, listMember{
					name: f.text[at : body+m[3]],
					line: strings.Count(f.text[:at], "\n") + 1,
				})
			}
			out = append(out, list)
		}
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

// constDecl is one string constant: the scope that declares it (a Go package directory or a Rust file), the module
// that a path to it names (the innermost inline module that holds it, else the module the scope is), and its value.
type constDecl struct {
	scope, module, value string
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

// pathModules returns, for each Rust file that a #[path = "..."] attribute loads, the names of the `mod NAME;`
// declarations that load it. The path is resolved as rustc resolves it: from the directory of the file that declares
// the module, or from the directory of its inline module (see declDir). A file that only a plain `mod NAME;` loads has
// no entry, because its stem, or its directory for a mod.rs, is NAME already.
func pathModules(rustFiles []sourceFile) map[string][]string {
	out := make(map[string][]string)
	for _, f := range rustFiles {
		lines := strings.Split(f.text, "\n")
		offset := 0
		for l, line := range lines {
			start := offset
			offset += len(line) + 1
			m := rustModDecl.FindStringSubmatch(strings.TrimSpace(line))
			if m == nil {
				continue
			}
			attr, ok := pathAttrAbove(lines, l)
			if !ok {
				continue
			}
			file := path.Join(declDir(f.rel, rustInlineMods(f.text), start), strings.ReplaceAll(attr, `\`, "/"))
			out[file] = append(out[file], m[1])
		}
	}
	return out
}

// declDir returns the directory a #[path] attribute on a module declaration at byte start of a Rust file is relative to.
// Outside an inline module that is the file's own directory. Inside one it is that directory, then the file's own name
// unless the file is a mod.rs or a crate root, then the name of each enclosing inline module, outermost first.
func declDir(rel string, mods []inlineMod, start int) string {
	dir := path.Dir(rel)
	chain := inlineChain(mods, start)
	if len(chain) == 0 {
		return dir
	}
	if base := path.Base(rel); base != "mod.rs" && base != "lib.rs" && base != "main.rs" {
		dir = path.Join(dir, strings.TrimSuffix(base, ".rs"))
	}
	return path.Join(append([]string{dir}, chain...)...)
}

// inlineChain returns the names of the inline modules whose braces enclose the byte at at, outermost first.
func inlineChain(mods []inlineMod, at int) []string {
	var enclosing []inlineMod
	for _, m := range mods {
		if m.open < at && at < m.end {
			enclosing = append(enclosing, m)
		}
	}
	slices.SortFunc(enclosing, func(a, b inlineMod) int { return a.open - b.open })
	names := make([]string, 0, len(enclosing))
	for _, m := range enclosing {
		names = append(names, m.name)
	}
	return names
}

// pathAttrAbove returns the path of the #[path] attribute that stacks over lines[l], if one does. Blank lines are skipped,
// because production text leaves its comment lines blank, and an attribute reaches its declaration across a comment. Any
// other line ends the stack.
func pathAttrAbove(lines []string, l int) (string, bool) {
	for above := l - 1; above >= 0; above-- {
		text := strings.TrimSpace(lines[above])
		if text == "" {
			continue
		}
		if !strings.HasPrefix(text, "#[") {
			return "", false
		}
		if m := rustPathAttr.FindStringSubmatch(text); m != nil {
			return m[1], true
		}
	}
	return "", false
}

// fileModules returns the module names that reach a Rust file's top-level constants: the names of the modules a #[path]
// attribute gives the file, or else the module name its stem gives.
func fileModules(pathMods map[string][]string, rel string) []string {
	if names := pathMods[rel]; len(names) > 0 {
		return names
	}
	return []string{scopeModule(rel)}
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
		if v, found, ambiguous := pick(decls, func(d constDecl) bool { return d.module == module }); ambiguous {
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
// `use ... as` renames of each Rust file. A constant at the top of a Rust file is reached through the module that a
// #[path] attribute gives the file, when one does, and otherwise through the file's own module name.
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
					c.byName[m[1]] = append(c.byName[m[1]], constDecl{scope: dir, module: path.Base(dir), value: m[2]})
				}
				continue
			}
			if m := goConstLine.FindStringSubmatch(f.text[at:min(at+300, len(f.text))]); m != nil {
				c.byName[m[1]] = append(c.byName[m[1]], constDecl{scope: dir, module: path.Base(dir), value: m[2]})
			}
		}
	}
	pathMods := pathModules(rustFiles)
	for _, f := range rustFiles {
		mods := rustInlineMods(f.text)
		for _, at := range wordStarts(f.text, "const ") {
			if m := rustConstLine.FindStringSubmatch(f.text[at:min(at+300, len(f.text))]); m != nil {
				modules := fileModules(pathMods, f.rel)
				if module := enclosingModule(mods, at); module != "" {
					modules = []string{module}
				}
				for _, module := range modules {
					c.byName[m[1]] = append(c.byName[m[1]], constDecl{scope: f.rel, module: module, value: m[2]})
				}
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

// productionSources returns the Rust sources as productionRust reads them, in the order given. The Go sources are
// left as they are.
func productionSources(files []sourceFile) []sourceFile {
	out := make([]sourceFile, len(files))
	for i, f := range files {
		out[i] = sourceFile{rel: f.rel, text: productionRust(f.text)}
	}
	return out
}

// productionRust returns a Rust source as the reader scan reads it: its #[cfg(test)] items and its comment lines are
// blanked, and every other line stays where it was, so the line numbers a finding reports stay right. A test module is
// not production code, so the strings it holds, which may spell a read, are not reads. Each item is blanked from its
// attribute through the last line that testItemEnd gives it.
func productionRust(text string) string {
	lines := strings.Split(text, "\n")
	for _, s := range testItemSpans(lines) {
		for k := s.first; k <= s.last; k++ {
			lines[k] = ""
		}
	}
	for i, line := range lines {
		if strings.HasPrefix(strings.TrimLeft(line, " \t"), "//") {
			lines[i] = ""
		}
	}
	return strings.Join(lines, "\n")
}

// testItemSpan is one #[cfg(test)] item: the line of its attribute, and the last line that it blanks.
type testItemSpan struct {
	first, last int
}

// testItemSpans returns the test items of lines in order. An item inside another item is part of that item, so only the
// outer one is listed. An attribute inside a raw string is text, not an attribute.
func testItemSpans(lines []string) []testItemSpan {
	var out []testItemSpan
	raw := -1
	for i := 0; i < len(lines); i++ {
		if raw < 0 && strings.HasPrefix(strings.TrimLeft(lines[i], " \t"), "#[cfg(test)]") {
			last := testItemEnd(lines, i)
			out = append(out, testItemSpan{first: i, last: last})
			i = last
			raw = rawStringOpen(lines[last], -1)
			continue
		}
		raw = rawStringOpen(lines[i], raw)
	}
	return out
}

// testItemEnd returns the last line of the test item that the attribute on line at opens. A head that is one item on its
// line ends there: a statement, a field, a variant or a match arm ends in ; or ,, and an item whose braces it closes ends in
// }. Any other head ends at the first line at the attribute's own indentation that starts with a closing brace, and a head
// that opens no brace also ends at a closing bracket or parenthesis there. The scan stops on the head alone at a line
// indented less than the attribute, because the attribute then sits in the last line of an enclosing block, and, for a head
// that opens no brace, at a line at the attribute's indentation that opens an item, because the next item has begun. The
// end of the text does too.
func testItemEnd(lines []string, at int) int {
	head, code := testItemHead(lines, at)
	if head < 0 {
		return at
	}
	if itemOnOneLine(code) {
		return head
	}
	indent := indentOf(lines[at])
	// A head that opens a brace ends at the brace at the attribute's indentation, so a fixture line that starts at column 0
	// inside it does not end it. Any other head also ends at a closing bracket or parenthesis, and it stops at the next item.
	block := strings.Contains(code, "{")
	raw := rawStringOpen(lines[head], -1)
	for k := head + 1; k < len(lines); k++ {
		inRaw := raw >= 0
		raw = rawStringOpen(lines[k], raw)
		if inRaw {
			// The line starts inside a raw string, so it is text: it neither ends the item nor stops the scan.
			continue
		}
		body := strings.TrimLeft(lines[k], " \t")
		if body == "" || strings.HasPrefix(body, "//") {
			continue
		}
		ind := indentOf(lines[k])
		switch {
		case ind < indent:
			return head
		case ind > indent:
			// A deeper line is inside the item.
		case strings.HasPrefix(body, "}"):
			return k
		case block:
		case (body[0] == ']' || body[0] == ')') && !strings.Contains(body, "{"):
			return k
		case opensItem(body):
			return head
		}
	}
	return head
}

// testItemHead returns the index of the line that holds the first token after the attribute on line at, and the code of
// that line with any // comment cut off. Blank lines, comment lines and further attributes are skipped, and an attribute
// that runs over several lines is skipped whole. It returns -1 when the text ends first.
func testItemHead(lines []string, at int) (int, string) {
	rest := strings.TrimPrefix(strings.TrimLeft(lines[at], " \t"), "#[cfg(test)]")
	open := 0
	for i := at; ; {
		code := strings.TrimSpace(codeBeforeComment(rest))
		switch {
		case open > 0:
			open += bracketBalance(code)
		case code == "":
		case strings.HasPrefix(code, "#["):
			open = bracketBalance(code)
		default:
			return i, code
		}
		i++
		if i == len(lines) {
			return -1, ""
		}
		rest = lines[i]
	}
}

// codeBeforeComment returns a line up to its // comment. A // inside a string literal is not a comment.
func codeBeforeComment(line string) string {
	inString := false
	for i := 0; i < len(line); i++ {
		c := line[i]
		switch {
		case inString && c == '\\':
			i++
		case c == '"':
			inString = !inString
		case !inString && c == '/' && i+1 < len(line) && line[i+1] == '/':
			return line[:i]
		}
	}
	return line
}

// rawStringOpen returns the number of # signs of the raw string that a line leaves open, or -1 when it leaves none open.
// open is the same for the text before the line, so a raw string that spans lines is followed line by line. A line comment
// ends the scan of its line, and a normal string or a character literal is passed over, so an r" inside either opens nothing.
func rawStringOpen(line string, open int) int {
	for i := 0; i < len(line); {
		if open >= 0 {
			closer := "\"" + strings.Repeat("#", open)
			k := strings.Index(line[i:], closer)
			if k < 0 {
				return open
			}
			i += k + len(closer)
			open = -1
			continue
		}
		c := line[i]
		switch {
		case c == '/' && i+1 < len(line) && line[i+1] == '/':
			return -1
		case c == '"':
			i = quotedEnd(line, i+1)
		case c == '\'':
			i = charLiteralEnd(line, i)
		case c == 'r' || c == 'b':
			if hashes, end, ok := rawPrefixAt(line, i); ok {
				open, i = hashes, end
			} else {
				i++
			}
		default:
			i++
		}
	}
	return open
}

// rawPrefixAt reports whether a raw string opener, r or br followed by hashes and a quote, starts at i. It returns the number
// of hashes and the index just past the quote. The r or b must not be the end of an identifier.
func rawPrefixAt(line string, i int) (int, int, bool) {
	if i > 0 && isIdentByte(line[i-1]) {
		return 0, 0, false
	}
	j := i
	if line[j] == 'b' {
		j++
	}
	if j >= len(line) || line[j] != 'r' {
		return 0, 0, false
	}
	j++
	hashes := 0
	for j < len(line) && line[j] == '#' {
		hashes++
		j++
	}
	if j < len(line) && line[j] == '"' {
		return hashes, j + 1, true
	}
	return 0, 0, false
}

// quotedEnd returns the index just past the quote that closes a string literal whose text starts at from. A string that does
// not close on its line ends with the line.
func quotedEnd(line string, from int) int {
	for j := from; j < len(line); j++ {
		switch line[j] {
		case '\\':
			j++
		case '"':
			return j + 1
		}
	}
	return len(line)
}

// charLiteralEnd returns the index just past the character literal whose opening quote is at i. A quote that does not open
// one is a lifetime's, and only the quote is passed over.
func charLiteralEnd(line string, i int) int {
	if i+1 < len(line) && line[i+1] == '\\' {
		from := min(i+3, len(line))
		if k := strings.IndexByte(line[from:], '\''); k >= 0 {
			return from + k + 1
		}
		return len(line)
	}
	if i+2 < len(line) && line[i+2] == '\'' {
		return i + 3
	}
	return i + 1
}

// bracketBalance returns how many square brackets text opens less the number it closes.
func bracketBalance(text string) int {
	return strings.Count(text, "[") - strings.Count(text, "]")
}

// indentOf returns the width of the spaces and tabs that start a line.
func indentOf(line string) int {
	return len(line) - len(strings.TrimLeft(line, " \t"))
}

// itemOpeners are the words that open an item. A line at a test attribute's own indentation that starts with one of them
// is the next item, so the test item ended on the line before it.
var itemOpeners = []string{
	"#[", "pub ", "pub(", "fn ", "async ", "unsafe ", "impl ", "mod ", "use ", "const ", "static ", "struct ", "enum ",
	"trait ", "type ", "extern ", "macro_rules!",
}

// opensItem reports whether a line starts with a word that opens an item.
func opensItem(body string) bool {
	return slices.ContainsFunc(itemOpeners, func(word string) bool { return strings.HasPrefix(body, word) })
}

// itemOnOneLine reports whether the item whose first line is head ends on that line: it ends in ; or ,, as a statement,
// a field, a variant or a match arm does, or its braces, if it has any, are closed there.
func itemOnOneLine(head string) bool {
	return strings.HasSuffix(head, ";") || strings.HasSuffix(head, ",") || (strings.Contains(head, "{") && strings.HasSuffix(head, "}"))
}

// inlineMod is an inline module, `mod name { ... }`: its name, and the byte offsets of its opening and closing braces.
type inlineMod struct {
	name      string
	open, end int
}

// rustInlineMods returns the inline modules of a Rust source. A module declared as `mod name;` has its body in
// another file, which is read as a module of its own, so it is not listed here.
func rustInlineMods(text string) []inlineMod {
	var out []inlineMod
	for _, at := range wordStarts(text, "mod ") {
		window := text[at:min(at+200, len(text))]
		m := rustInlineModHead.FindStringSubmatchIndex(window)
		if m == nil {
			continue
		}
		open := at + m[1] - 1
		end := closingIndex(text, open, '{', '}')
		if end < 0 {
			continue
		}
		out = append(out, inlineMod{name: group(window, m, 1), open: open, end: end})
	}
	return out
}

// enclosingModule returns the name of the innermost inline module whose braces enclose the byte at at, or "" when
// no inline module does.
func enclosingModule(mods []inlineMod, at int) string {
	best, name := -1, ""
	for _, m := range mods {
		if m.open < at && at < m.end && m.open > best {
			best, name = m.open, m.name
		}
	}
	return name
}

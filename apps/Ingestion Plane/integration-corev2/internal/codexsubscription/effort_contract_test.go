package codexsubscription

// Pins the reasoning-effort contract between this broker and Model Plane's
// inference-core, whose ladders (CODEX_LEVELS and CODEX_WIDE_LEVELS in effort.rs)
// decide which reasoning efforts it sends the broker.
//
// The two are different languages and deploy separately, so neither side can
// import the other's constants. This test therefore reads inference-core's
// source for the ladders, and asserts the broker half at runtime against the
// real acceptsReasoningEffort. TestEffortContractGoesRedWhenEitherSideDrifts
// shows that a drift on either side fails here. It closes D1-725 and D1-730.

import (
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"testing"
)

var (
	// Anchored at the start of a line, so the prose comments in effort.rs that
	// name these constants never match.
	codexLevelsDecl     = regexp.MustCompile(`(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?const\s+CODEX_LEVELS\s*:\s*&\[Level\]\s*=\s*&\[([^\]]*)\]\s*;`)
	codexWideLevelsDecl = regexp.MustCompile(`(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?const\s+CODEX_WIDE_LEVELS\s*:\s*&\[Level\]\s*=\s*&\[([^\]]*)\]\s*;`)
	levelVariant        = regexp.MustCompile(`Level::(\w+)`)
	asStrBody           = regexp.MustCompile(`pub fn as_str\(self\) -> &'static str \{([\s\S]*?)\n    \}`)
	wireNameArm         = regexp.MustCompile(`Level::(\w+)\s*=>\s*"([a-z]+)"`)
)

// readEffortSource returns inference-core's effort.rs with CRLF folded to LF.
// It skips when the Model Plane tree is not beside this module, and fails when
// the file has moved, so a moved file cannot quietly disable the contract.
func readEffortSource(t *testing.T) string {
	t.Helper()
	effortModelPlaneDir := filepath.Join("..", "..", "..", "..", "Model Plane")
	if _, err := os.Stat(effortModelPlaneDir); err != nil {
		t.Skipf("Model Plane is not beside Ingestion Plane (%v); the broker/inference-core effort contract cannot be checked here", err)
	}
	path := filepath.Join(effortModelPlaneDir, "rust", "services", "inference-core", "src", "provider", "effort.rs")
	source, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("cannot read %s (%v). If the file moved, re-point effortModelPlaneDir; do not delete this test: it is what keeps the broker and inference-core on one effort ladder.", path, err)
	}
	return strings.ReplaceAll(string(source), "\r\n", "\n")
}

// parseLevelWireNames maps each Level variant to the wire name Level::as_str
// gives it. The source must hold exactly one as_str body.
func parseLevelWireNames(t *testing.T, source string) map[string]string {
	t.Helper()
	bodies := asStrBody.FindAllStringSubmatch(source, -1)
	if len(bodies) != 1 {
		t.Fatalf("effort.rs has %d `pub fn as_str(self) -> &'static str` bodies, want exactly one", len(bodies))
	}
	wires := map[string]string{}
	for _, arm := range wireNameArm.FindAllStringSubmatch(bodies[0][1], -1) {
		wires[arm[1]] = arm[2]
	}
	return wires
}

// parseLadder returns one ladder constant's wire names in declaration order. It
// fails when the declaration is missing or repeated, when the list is empty, or
// when a listed variant has no wire name.
func parseLadder(t *testing.T, source string, decl *regexp.Regexp, name string, wires map[string]string) []string {
	t.Helper()
	matches := decl.FindAllStringSubmatch(source, -1)
	if len(matches) != 1 {
		t.Fatalf("%s: effort.rs has %d declarations, want exactly one; if the constant moved, re-point the pattern", name, len(matches))
	}
	variants := levelVariant.FindAllStringSubmatch(matches[0][1], -1)
	if len(variants) == 0 {
		t.Fatalf("%s is empty in effort.rs", name)
	}
	ladder := make([]string, 0, len(variants))
	for _, variant := range variants {
		wire, ok := wires[variant[1]]
		if !ok {
			t.Fatalf("%s lists Level::%s, which Level::as_str does not map", name, variant[1])
		}
		ladder = append(ladder, wire)
	}
	return ladder
}

// parseEffortContract returns the two broker ladders and the universe of
// reasoning-effort spellings the broker is checked against: every wire name
// as_str renders, plus spellings no side sends and the broker must refuse.
func parseEffortContract(t *testing.T, source string) (codex, codexWide, universe []string) {
	t.Helper()
	wires := parseLevelWireNames(t, source)
	codex = parseLadder(t, source, codexLevelsDecl, "CODEX_LEVELS", wires)
	codexWide = parseLadder(t, source, codexWideLevelsDecl, "CODEX_WIDE_LEVELS", wires)
	for _, wire := range wires {
		universe = append(universe, wire)
	}
	slices.Sort(universe)
	universe = append(universe, "maximum", "none", "ultra", "")
	return codex, codexWide, universe
}

// effortViolations reports every ladder value the broker rejects, and every
// universe value the broker admits that the ladder does not list. An empty
// result means the broker and the ladder agree.
func effortViolations(ladder []string, accepts func(string) bool, universe []string) []string {
	var violations []string
	listed := make(map[string]bool, len(ladder))
	for _, wire := range ladder {
		listed[wire] = true
		if !accepts(wire) {
			violations = append(violations, wire)
		}
	}
	for _, wire := range universe {
		if accepts(wire) && !listed[wire] {
			violations = append(violations, wire)
		}
	}
	return violations
}

// newEffortContractBroker returns a broker with the wide switch set as given,
// and the runner its invocations reach.
func newEffortContractBroker(t *testing.T, wide bool) (*Manager, *fakeRunner) {
	t.Helper()
	runner := &fakeRunner{}
	manager, err := NewManager(Config{Enabled: true, Home: t.TempDir(), WideReasoningEffort: wide}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	return manager, runner
}

// With the wide switch off, the broker must admit exactly CODEX_LEVELS.
func TestBrokerPairMatchesInferenceCoreCodexLevels(t *testing.T) {
	codex, _, universe := parseEffortContract(t, readEffortSource(t))
	narrow, _ := newEffortContractBroker(t, false)
	if violations := effortViolations(codex, narrow.acceptsReasoningEffort, universe); len(violations) != 0 {
		t.Fatalf("switch off: broker disagrees with CODEX_LEVELS %v on %v", codex, violations)
	}
}

// With the wide switch on, the broker must admit exactly CODEX_WIDE_LEVELS, the
// ladder inference-core sends once its own wide switch is on.
func TestWideBrokerMatchesInferenceCoreCodexWideLevels(t *testing.T) {
	_, codexWide, universe := parseEffortContract(t, readEffortSource(t))
	wide, _ := newEffortContractBroker(t, true)
	if violations := effortViolations(codexWide, wide.acceptsReasoningEffort, universe); len(violations) != 0 {
		t.Fatalf("switch on: broker disagrees with CODEX_WIDE_LEVELS %v on %v", codexWide, violations)
	}
}

// An empty effort is defaulted to "low" by prepareInvocation, and inference-core
// falls back to "low" too (codex_reasoning_effort). The broker's default must
// therefore be a value the ladder lists, and it must reach Codex unchanged.
func TestBrokerDefaultEffortIsOnTheInferenceCoreLadder(t *testing.T) {
	codex, _, _ := parseEffortContract(t, readEffortSource(t))
	if !slices.Contains(codex, "low") {
		t.Fatalf("CODEX_LEVELS %v does not list low, the broker's default effort", codex)
	}
	narrow, runner := newEffortContractBroker(t, false)
	if !narrow.acceptsReasoningEffort("low") {
		t.Fatal("switch off: the broker rejects low, its default effort")
	}
	_, err := narrow.Invoke(t.Context(), InvokeRequest{ConnectionID: "conn-1", Model: "gpt-5", Messages: []ChatMessage{{Role: "user", Content: "hello"}}})
	if err != nil {
		t.Fatalf("Invoke without an effort: %v", err)
	}
	if got := runner.invokeRequest.ReasoningEffort; got != "low" {
		t.Fatalf("broker forwarded %q for an empty effort, want low", got)
	}
}

// mutate returns text with its single occurrence of from replaced by to. The
// anchor must occur exactly once, or the mutation would test nothing.
func mutate(t *testing.T, text, from, to string) string {
	t.Helper()
	if n := strings.Count(text, from); n != 1 {
		t.Fatalf("mutation anchor occurs %d times, want exactly once: %q", n, from)
	}
	return strings.Replace(text, from, to, 1)
}

// Each mutation changes one side the way a real edit would, and the comparison
// must report it. The anchor is the whole CODEX_WIDE_LEVELS line: fragments of
// it also appear in effort.rs's own tests.
func TestEffortContractGoesRedWhenEitherSideDrifts(t *testing.T) {
	source := readEffortSource(t)
	codex, codexWide, universe := parseEffortContract(t, source)
	narrow, _ := newEffortContractBroker(t, false)
	wide, _ := newEffortContractBroker(t, true)
	if v := effortViolations(codex, narrow.acceptsReasoningEffort, universe); len(v) != 0 {
		t.Fatalf("baseline: switch-off broker disagrees with CODEX_LEVELS on %v", v)
	}
	if v := effortViolations(codexWide, wide.acceptsReasoningEffort, universe); len(v) != 0 {
		t.Fatalf("baseline: switch-on broker disagrees with CODEX_WIDE_LEVELS on %v", v)
	}

	// inference-core drops xhigh from its wide ladder while the wide broker still
	// admits it.
	wideLine := `const CODEX_WIDE_LEVELS: &[Level] = &[Level::Low, Level::Medium, Level::High, Level::XHigh];`
	droppedXHigh := mutate(t, source, wideLine, `const CODEX_WIDE_LEVELS: &[Level] = &[Level::Low, Level::Medium, Level::High];`)
	_, shortWide, _ := parseEffortContract(t, droppedXHigh)
	if v := effortViolations(shortWide, wide.acceptsReasoningEffort, universe); !slices.Contains(v, "xhigh") {
		t.Fatalf("a wide ladder without xhigh should disagree with the wide broker on xhigh; got %v", v)
	}

	// The broker starts admitting max, which no ladder lists.
	admitsMax := func(effort string) bool { return wide.acceptsReasoningEffort(effort) || effort == "max" }
	if v := effortViolations(codexWide, admitsMax, universe); !slices.Contains(v, "max") {
		t.Fatalf("a broker that admits max should disagree with CODEX_WIDE_LEVELS on max; got %v", v)
	}

	// The wide broker stops admitting medium, which the wide ladder still sends.
	dropsMedium := func(effort string) bool { return effort != "medium" && wide.acceptsReasoningEffort(effort) }
	if v := effortViolations(codexWide, dropsMedium, universe); !slices.Contains(v, "medium") {
		t.Fatalf("a wide broker that drops medium should disagree with CODEX_WIDE_LEVELS on medium; got %v", v)
	}
}

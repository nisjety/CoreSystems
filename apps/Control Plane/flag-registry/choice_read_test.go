package flagregistry

import (
	"errors"
	"slices"
	"strings"
	"testing"
)

// choiceReadPin is one shipped choice and how its service reads the raw variable. Each reader was traced by
// hand and is named here. A new choice must be pinned with its reader, so the registry never describes a
// choice whose reader nobody traced.
type choiceReadPin struct {
	name   string
	read   ChoiceRead
	reader string
}

var choiceReadPins = []choiceReadPin{
	{"CLIENT_VERSION_POLICY", ChoiceReadAnyCase, "gateway client_contract.rs: ClientVersionPolicy::parse trims and lowercases"},
	{"CONTENT_WRITE_POLICY", ChoiceReadExact, "session-core write_policy.rs: enforce and report are exact; a padded off reads as off in the service and is refused here"},
	{"CONTROL_PLANE_ENFORCEMENT", ChoiceReadAnyCase, "gateway config.rs: trims and lowercases (retrieval-engine reads the same name exactly; see the README)"},
	{"CRAWL_APPROVAL_MODE", ChoiceReadAnyCase, "gateway domains/knowledge.rs: parse_crawl_approval_mode trims and lowercases"},
	{"EXECUTION_CORE_SPACE_APPROVAL_GATE", ChoiceReadAnyCase, "execution-core space_gate.rs: GateMode::parse trims and lowercases"},
	{"LETTA_TOOL_SEARCH_MODE", "", "capability-core lettatools client.go: TrimSpace, then an exact match against vector, fts and hybrid"},
	{"MCP_DO_TOOLS", ChoiceReadExact, "execution-core mcp_gateway.rs: from_raw matches read and all exactly"},
	{"MODEL_ACTION_ADMISSION", ChoiceReadExact, "capability-core model_action_view.go (os.Getenv) and execution-core ticket_tools.rs and owner_reads.rs: the literal table, exactly"},
	{"MODEL_GATEWAY_AI_CHAT_METERING", ChoiceReadExact, "model-gateway http_routes.rs: AiChatMetering::parse matches service and all exactly, with no trimming"},
	{"MODEL_GATEWAY_ARTIFACT_REVISIONS", ChoiceReadExact, "model-gateway artifact_revisions.rs: ArtifactRevisionsMode::parse matches off, shadow and on exactly; unset and empty are off, and any other value refuses boot"},
	{"MODEL_GATEWAY_REVIEW_PROPOSAL_RULE", ChoiceReadExact, "model-gateway review_proposal_rule.rs: recognise matches off and on exactly, with no folding or trimming"},
	{"PROMOTE_ON_USE_SIGNAL", ChoiceReadExact, "execution-core promote_on_use.rs: the variable must equal cited exactly"},
	{"RUN_ACTION_TARGETS", ChoiceReadExact, "user-core run_action_targets.go: RunActionTargetsModeFromLookup compares the raw value with table, exactly; anything else keeps tickets_only"},
	{"SESSION_RETENTION_SWEEP", ChoiceReadExact, "session-core retention_sweeper.rs: mode_from matches enforce exactly and finds off after trimming and in any case"},
	{"TASK_EXECUTOR_ENABLED", ChoiceReadExact, "capability-core cmd/main.go: taskExecutorEnabled matches true exactly; the service also reads a padded or cased false as off, which the typed read refuses"},
	{"WORKSPACE_TOOLS", ChoiceReadExact, "execution-core workspace_tools.rs: parse_mode matches read and all exactly"},
}

// TestChoiceReadPinsTraceEveryShippedChoice keeps the pins and the shipped choices the same set, and each
// choice's read mode the one its reader takes.
func TestChoiceReadPinsTraceEveryShippedChoice(t *testing.T) {
	reg := loadShipped(t)
	var shipped []string
	for _, f := range reg.Flags() {
		if f.Kind == KindChoice {
			shipped = append(shipped, f.Name)
		}
	}
	var pinned []string
	for _, p := range choiceReadPins {
		pinned = append(pinned, p.name)
		f, ok := reg.Flag(p.name)
		if !ok {
			t.Errorf("%s is pinned but has no registry entry", p.name)
			continue
		}
		if f.Kind != KindChoice {
			t.Errorf("%s: kind %q, want choice", p.name, f.Kind)
			continue
		}
		if f.ChoiceRead != p.read {
			t.Errorf("%s: choiceRead %q, want %q (%s)", p.name, f.ChoiceRead, p.read, p.reader)
		}
	}
	slices.Sort(pinned)
	if !slices.Equal(shipped, pinned) {
		t.Errorf("shipped choices %v are not the pinned set %v; trace the new choice's reader and pin it", shipped, pinned)
	}
}

// TestExactChoicesRefuseAPaddedOrCasedValue pins the exact readers: a value that is not the choice as written,
// padded or in another case, is refused, never trimmed or folded into a choice.
func TestExactChoicesRefuseAPaddedOrCasedValue(t *testing.T) {
	reg := loadShipped(t)
	for _, p := range choiceReadPins {
		if p.read != ChoiceReadExact {
			continue
		}
		f, _ := reg.Flag(p.name)
		for _, v := range f.Values {
			if v == "" {
				continue
			}
			got, err := reg.Choice(p.name, lookupFrom(map[string]string{p.name: v}))
			if err != nil || got != v {
				t.Errorf("%s with %q = %q, %v; want %q", p.name, v, got, err, v)
			}
			padded := " " + v + " "
			if _, err := reg.Choice(p.name, lookupFrom(map[string]string{p.name: padded})); !errors.Is(err, ErrUnknownChoice) {
				t.Errorf("%s with %q: error %v, want ErrUnknownChoice", p.name, padded, err)
			}
			if upper := strings.ToUpper(v); upper != v {
				if _, err := reg.Choice(p.name, lookupFrom(map[string]string{p.name: upper})); !errors.Is(err, ErrUnknownChoice) {
					t.Errorf("%s with %q: error %v, want ErrUnknownChoice", p.name, upper, err)
				}
			}
		}
	}
}

// TestAnyCaseChoicesTrimAndFold pins the folded readers: surrounding space and any case of a choice read as the
// choice, and a value outside Values is still refused.
func TestAnyCaseChoicesTrimAndFold(t *testing.T) {
	reg := loadShipped(t)
	for _, p := range choiceReadPins {
		if p.read != ChoiceReadAnyCase {
			continue
		}
		f, _ := reg.Flag(p.name)
		for _, v := range f.Values {
			if v == "" {
				continue
			}
			raw := "\t" + strings.ToUpper(v) + " "
			got, err := reg.Choice(p.name, lookupFrom(map[string]string{p.name: raw}))
			if err != nil || got != v {
				t.Errorf("%s with %q = %q, %v; want %q", p.name, raw, got, err, v)
			}
		}
		if _, err := reg.Choice(p.name, lookupFrom(map[string]string{p.name: "magic"})); !errors.Is(err, ErrUnknownChoice) {
			t.Errorf("%s with magic: error %v, want ErrUnknownChoice", p.name, err)
		}
	}
}

// TestTrimmedChoiceTrimsButKeepsCase pins the default read, which the service takes for LETTA_TOOL_SEARCH_MODE:
// surrounding space is removed, and case is not folded, so a cased value is not a choice.
func TestTrimmedChoiceTrimsButKeepsCase(t *testing.T) {
	reg := loadShipped(t)
	const name = "LETTA_TOOL_SEARCH_MODE"
	got, err := reg.Choice(name, lookupFrom(map[string]string{name: " fts "}))
	if err != nil || got != "fts" {
		t.Fatalf("%s with \" fts \" = %q, %v; want fts", name, got, err)
	}
	if _, err := reg.Choice(name, lookupFrom(map[string]string{name: "FTS"})); !errors.Is(err, ErrUnknownChoice) {
		t.Fatalf("%s with FTS: error %v, want ErrUnknownChoice", name, err)
	}
}

// TestBlankChoiceValueIsWhatTheReaderSees pins the blank rule. A variable that is only whitespace is not the
// default for an exact or any-case choice: its service reads the blank value itself. EXECUTION_CORE_SPACE_APPROVAL_GATE
// reads blank as off, so the typed read refuses it rather than returning the enforce default. A trimmed choice
// still takes its default, as LETTA_TOOL_SEARCH_MODE does. An unset or empty variable takes the default for every
// choice, as compose's ${NAME:-default} does.
func TestBlankChoiceValueIsWhatTheReaderSees(t *testing.T) {
	reg := loadShipped(t)
	const gate = "EXECUTION_CORE_SPACE_APPROVAL_GATE"
	if _, err := reg.Choice(gate, lookupFrom(map[string]string{gate: "   "})); !errors.Is(err, ErrUnknownChoice) {
		t.Errorf("%s with blank space: error %v, want ErrUnknownChoice (the service reads it as off)", gate, err)
	}
	if got, err := reg.Choice(gate, lookupFrom(map[string]string{gate: ""})); err != nil || got != "enforce" {
		t.Errorf("%s with empty = %q, %v; want the enforce default", gate, got, err)
	}
	if got, err := reg.Choice(gate, nil); err != nil || got != "enforce" {
		t.Errorf("%s unset = %q, %v; want the enforce default", gate, got, err)
	}
	const letta = "LETTA_TOOL_SEARCH_MODE"
	if got, err := reg.Choice(letta, lookupFrom(map[string]string{letta: "   "})); err != nil || got != "hybrid" {
		t.Errorf("%s with blank space = %q, %v; want the hybrid default", letta, got, err)
	}
	const admission = "MODEL_ACTION_ADMISSION"
	if _, err := reg.Choice(admission, lookupFrom(map[string]string{admission: "   "})); !errors.Is(err, ErrUnknownChoice) {
		t.Errorf("%s with blank space: error %v, want ErrUnknownChoice", admission, err)
	}
	if got, err := reg.Choice(admission, lookupFrom(map[string]string{admission: ""})); err != nil || got != "" {
		t.Errorf("%s with empty = %q, %v; want the empty default", admission, got, err)
	}
}

// TestChoiceReadRejectsALooserOrUnknownMode keeps the field honest: a value other than exact or anyCase fails
// the load, an any-case choice lists only lowercase values, a trimmed or any-case choice lists no value with
// surrounding space, and the field applies only to choices.
func TestChoiceReadRejectsALooserOrUnknownMode(t *testing.T) {
	choice := func() Flag {
		return Flag{
			Name:         "EXAMPLE_MODE",
			Kind:         KindChoice,
			Owner:        PlaneModel,
			Source:       SourceCompose,
			Default:      "off",
			Values:       []string{"off", "on"},
			Purpose:      "Example choice.",
			Declarations: []Declaration{{Service: "model-gateway", Env: "EXAMPLE_MODE", Default: "off"}},
		}
	}
	cases := []struct {
		name string
		flag func() Flag
		want string
	}{
		{"unknown mode", func() Flag {
			f := choice()
			f.ChoiceRead = "loose"
			return f
		}, `choiceRead "loose" must be exact or anyCase`},
		{"mode on a switch", func() Flag {
			f := exampleSwitch()
			f.ChoiceRead = ChoiceReadExact
			return f
		}, "choiceRead applies only to choice flags"},
		{"any-case value with a capital", func() Flag {
			f := choice()
			f.ChoiceRead = ChoiceReadAnyCase
			f.Values = []string{"off", "On"}
			return f
		}, `choiceRead anyCase needs lowercase values, and "On" is not`},
		{"trimmed value with space", func() Flag {
			f := choice()
			f.Values = []string{"off", " on"}
			return f
		}, `value " on" has surrounding space`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := New(exampleFile(tc.flag()))
			if err == nil {
				t.Fatalf("New accepted the document; want an error containing %q", tc.want)
			}
			if !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("error %q does not contain %q", err, tc.want)
			}
		})
	}
}

package flagregistry

import (
	"errors"
	"strings"
	"testing"
)

func lookupFrom(values map[string]string) Lookup {
	return func(name string) (string, bool) {
		v, ok := values[name]
		return v, ok
	}
}

func resolveRegistry(t *testing.T) *Registry {
	t.Helper()
	text := Flag{
		Name:     "EXAMPLE_LIMIT",
		Kind:     KindText,
		Owner:    PlaneModel,
		Source:   SourceCode,
		Default:  "",
		Purpose:  "Example text knob for the registry tests.",
		CodePath: "apps/Model Plane/README.md",
	}
	choice := Flag{
		Name:         "EXAMPLE_MODE",
		Kind:         KindChoice,
		Owner:        PlaneModel,
		Source:       SourceCompose,
		Default:      "hybrid",
		Values:       []string{"vector", "fts", "hybrid"},
		Purpose:      "Example choice knob for the registry tests.",
		Declarations: []Declaration{{Service: "capability-core", Env: "EXAMPLE_MODE", Default: "hybrid"}},
	}
	kill := Flag{
		Name:            "KILL_SWITCH_ENABLED",
		Kind:            KindSwitch,
		Owner:           PlaneModel,
		Source:          SourceCode,
		Default:         "true",
		Purpose:         "Example kill switch that is on by default.",
		OnDefaultReason: "Kill switch: on unless an operator turns it off.",
		CodePath:        "apps/Model Plane/README.md",
	}
	reg, err := New(exampleFile(exampleSwitch(), text, choice, kill))
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return reg
}

func TestSwitchTakesTheDefaultWhenUnsetOrBlank(t *testing.T) {
	reg := resolveRegistry(t)
	cases := map[string]Lookup{
		"nil lookup": nil,
		"unset":      lookupFrom(nil),
		"blank":      lookupFrom(map[string]string{"EXAMPLE_ENABLED": "   ", "KILL_SWITCH_ENABLED": ""}),
	}
	for name, lookup := range cases {
		t.Run(name, func(t *testing.T) {
			off, err := reg.Switch("EXAMPLE_ENABLED", lookup)
			if err != nil || off {
				t.Fatalf("EXAMPLE_ENABLED = %t, %v; want the off default", off, err)
			}
			on, err := reg.Switch("KILL_SWITCH_ENABLED", lookup)
			if err != nil || !on {
				t.Fatalf("KILL_SWITCH_ENABLED = %t, %v; want the on default", on, err)
			}
		})
	}
}

func TestSwitchReadsSetValues(t *testing.T) {
	reg := resolveRegistry(t)
	lookup := lookupFrom(map[string]string{"EXAMPLE_ENABLED": " Yes ", "KILL_SWITCH_ENABLED": "off"})
	on, err := reg.Switch("EXAMPLE_ENABLED", lookup)
	if err != nil || !on {
		t.Fatalf("EXAMPLE_ENABLED = %t, %v; want true", on, err)
	}
	kill, err := reg.Switch("KILL_SWITCH_ENABLED", lookup)
	if err != nil || kill {
		t.Fatalf("KILL_SWITCH_ENABLED = %t, %v; want false", kill, err)
	}
}

func TestSwitchRejectsUnknownValues(t *testing.T) {
	reg := resolveRegistry(t)
	_, err := reg.Switch("EXAMPLE_ENABLED", lookupFrom(map[string]string{"EXAMPLE_ENABLED": "maybe"}))
	if !errors.Is(err, ErrUnknownSwitchValue) {
		t.Fatalf("error = %v, want ErrUnknownSwitchValue", err)
	}
	if !strings.Contains(err.Error(), "EXAMPLE_ENABLED") {
		t.Fatalf("error %q does not name the variable", err)
	}
}

func TestTypedReadsRejectUnknownNamesAndWrongKinds(t *testing.T) {
	reg := resolveRegistry(t)
	if _, err := reg.Switch("NOT_REGISTERED_ENABLED", nil); !errors.Is(err, ErrUnknownFlag) {
		t.Fatalf("unknown name: error = %v, want ErrUnknownFlag", err)
	}
	if _, err := reg.Switch("EXAMPLE_MODE", nil); !errors.Is(err, ErrWrongKind) {
		t.Fatalf("Switch on a choice: error = %v, want ErrWrongKind", err)
	}
	if _, err := reg.Choice("EXAMPLE_ENABLED", nil); !errors.Is(err, ErrWrongKind) {
		t.Fatalf("Choice on a switch: error = %v, want ErrWrongKind", err)
	}
	if _, err := reg.Text("EXAMPLE_MODE", nil); !errors.Is(err, ErrWrongKind) {
		t.Fatalf("Text on a choice: error = %v, want ErrWrongKind", err)
	}
}

func TestChoiceAcceptsDeclaredValuesOnly(t *testing.T) {
	reg := resolveRegistry(t)
	got, err := reg.Choice("EXAMPLE_MODE", nil)
	if err != nil || got != "hybrid" {
		t.Fatalf("default = %q, %v; want hybrid", got, err)
	}
	got, err = reg.Choice("EXAMPLE_MODE", lookupFrom(map[string]string{"EXAMPLE_MODE": " vector "}))
	if err != nil || got != "vector" {
		t.Fatalf("set value = %q, %v; want vector", got, err)
	}
	_, err = reg.Choice("EXAMPLE_MODE", lookupFrom(map[string]string{"EXAMPLE_MODE": "magic"}))
	if !errors.Is(err, ErrUnknownChoice) {
		t.Fatalf("error = %v, want ErrUnknownChoice", err)
	}
}

func TestTextReturnsTrimmedValueOrDefault(t *testing.T) {
	reg := resolveRegistry(t)
	got, err := reg.Text("EXAMPLE_LIMIT", nil)
	if err != nil || got != "" {
		t.Fatalf("default = %q, %v; want empty", got, err)
	}
	got, err = reg.Text("EXAMPLE_LIMIT", lookupFrom(map[string]string{"EXAMPLE_LIMIT": "  gpt-5  "}))
	if err != nil || got != "gpt-5" {
		t.Fatalf("set value = %q, %v; want gpt-5", got, err)
	}
}

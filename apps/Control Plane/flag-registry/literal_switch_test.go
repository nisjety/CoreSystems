package flagregistry

import (
	"errors"
	"slices"
	"strings"
	"testing"
)

// literalFlags are three switches of the exact kind: the literal true on a default-off gate, the
// literal true on a default-on switch, and the literal 1 on a gate that reads 1. Their typed reads
// must agree with the service readers, which compare the raw variable with the literal.
func literalFlags() []Flag {
	trueOff := exampleSwitch()
	trueOff.Name = "EXAMPLE_TRUE_ENABLED"
	trueOff.Purpose = "Example exact switch. Only the literal true turns it on."
	trueOff.OnLiteral = "true"
	trueOff.Default = "0"
	trueOff.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_TRUE_ENABLED", Default: "0"}}

	trueOn := exampleSwitch()
	trueOn.Name = "EXAMPLE_TRUE_ON_ENABLED"
	trueOn.Purpose = "Example exact switch on by default. Only the literal true turns it on."
	trueOn.OnLiteral = "true"
	trueOn.Default = "true"
	trueOn.OnDefaultReason = "Example: compose turns it on."
	trueOn.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_TRUE_ON_ENABLED", Default: "true"}}

	gate := exampleSwitch()
	gate.Name = "EXAMPLE_GATE_ENABLED"
	gate.Purpose = "Example gate. Only the literal 1 turns it on."
	gate.OnLiteral = "1"
	gate.Default = "0"
	gate.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_GATE_ENABLED", Default: "0"}}

	return []Flag{gate, trueOff, trueOn}
}

// TestOnLiteralSwitchReadsOnlyItsLiteral pins the typed read of an exact switch to the service's
// reader: only the raw literal turns it on, a padded, cased or other spelling reads as off, and an
// unset or empty variable takes the default.
func TestOnLiteralSwitchReadsOnlyItsLiteral(t *testing.T) {
	reg, err := New(exampleFile(literalFlags()...))
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	cases := []struct {
		name  string
		value string
		set   bool
		want  bool
		why   string
	}{
		{"EXAMPLE_TRUE_ENABLED", "", false, false, "unset takes the off default"},
		{"EXAMPLE_TRUE_ENABLED", "true", true, true, "the literal turns it on"},
		{"EXAMPLE_TRUE_ENABLED", "TRUE", true, false, "a cased spelling is off"},
		{"EXAMPLE_TRUE_ENABLED", " true ", true, false, "a padded spelling is off"},
		{"EXAMPLE_TRUE_ENABLED", "1", true, false, "1 is off for the literal true"},
		{"EXAMPLE_TRUE_ENABLED", "yes", true, false, "yes is off"},
		{"EXAMPLE_TRUE_ENABLED", "on", true, false, "on is off"},
		{"EXAMPLE_TRUE_ENABLED", "enabled", true, false, "enabled is off"},
		{"EXAMPLE_TRUE_ENABLED", "", true, false, "an empty variable takes the off default"},
		{"EXAMPLE_TRUE_ENABLED", "   ", true, false, "whitespace is a value the service reads as off"},
		{"EXAMPLE_TRUE_ON_ENABLED", "", false, true, "unset takes the on default"},
		{"EXAMPLE_TRUE_ON_ENABLED", "", true, true, "an empty variable takes the on default"},
		{"EXAMPLE_TRUE_ON_ENABLED", "   ", true, false, "whitespace is not empty, so it is not the default"},
		{"EXAMPLE_TRUE_ON_ENABLED", "0", true, false, "0 turns the default-on switch off"},
		{"EXAMPLE_TRUE_ON_ENABLED", "1", true, false, "1 turns the default-on switch off, as the service does"},
		{"EXAMPLE_TRUE_ON_ENABLED", "true", true, true, "the literal keeps it on"},
		{"EXAMPLE_GATE_ENABLED", "1", true, true, "the literal 1 opens the gate"},
		{"EXAMPLE_GATE_ENABLED", "true", true, false, "true does not open a gate that reads 1"},
		{"EXAMPLE_GATE_ENABLED", "", false, false, "unset takes the off default"},
	}
	for _, tc := range cases {
		t.Run(tc.name+"/"+tc.why, func(t *testing.T) {
			lookup := lookupFrom(nil)
			if tc.set {
				lookup = lookupFrom(map[string]string{tc.name: tc.value})
			}
			got, err := reg.Switch(tc.name, lookup)
			if err != nil {
				t.Fatalf("Switch(%s) with %q: unexpected error %v", tc.name, tc.value, err)
			}
			if got != tc.want {
				t.Fatalf("Switch(%s) with %q = %t, want %t", tc.name, tc.value, got, tc.want)
			}
		})
	}
}

// TestCanonicalSwitchStillReadsTheParseSwitchVocabulary keeps the canonical read unchanged: a
// switch without an exact literal still reads 1, on and yes as on, trims, and rejects a typo.
func TestCanonicalSwitchStillReadsTheParseSwitchVocabulary(t *testing.T) {
	reg := resolveRegistry(t)
	for _, raw := range []string{"1", " Yes ", "on", "enabled"} {
		on, err := reg.Switch("EXAMPLE_ENABLED", lookupFrom(map[string]string{"EXAMPLE_ENABLED": raw}))
		if err != nil || !on {
			t.Fatalf("EXAMPLE_ENABLED with %q = %t, %v; want on", raw, on, err)
		}
	}
	if _, err := reg.Switch("EXAMPLE_ENABLED", lookupFrom(map[string]string{"EXAMPLE_ENABLED": "ture"})); !errors.Is(err, ErrUnknownSwitchValue) {
		t.Fatalf("a typo: error = %v, want ErrUnknownSwitchValue", err)
	}
}

// TestOnLiteralRejectsALooserVocabulary makes the registry's own tests fail when a row for an exact
// switch names a different literal, declares a looser spelling as a default, or carries the field on
// a kind that cannot have it.
func TestOnLiteralRejectsALooserVocabulary(t *testing.T) {
	base := func() Flag {
		f := exampleSwitch()
		f.Purpose = "Example exact switch. Only the literal true turns it on."
		f.OnLiteral = "true"
		return f
	}
	cases := []struct {
		name string
		flag func() Flag
		want string
	}{
		{"literal and any-case literal both set", func() Flag {
			f := base()
			f.OnLiteralAnyCase = "true"
			return f
		}, "set onLiteral or onLiteralAnyCase, not both"},
		{"any-case literal is not true", func() Flag {
			f := base()
			f.OnLiteral = ""
			f.OnLiteralAnyCase = "1"
			f.Purpose = "Example switch. Only the literal true, in any case, turns it on."
			return f
		}, "onLiteralAnyCase \"1\" must be true"},
		{"any-case purpose does not say in any case", func() Flag {
			f := base()
			f.OnLiteral = ""
			f.OnLiteralAnyCase = "true"
			return f
		}, "the purpose must say that only the literal true, in any case, turns it on"},
		{"any-case default-on switch declares TRUE", func() Flag {
			f := base()
			f.OnLiteral = ""
			f.OnLiteralAnyCase = "true"
			f.Purpose = "Example switch. Only the literal true, in any case, turns it on."
			f.Default = "TRUE"
			f.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_ENABLED", Default: "TRUE"}}
			f.OnDefaultReason = "Example."
			return f
		}, "the value \"TRUE\" reads as on, but only the exact value true turns the switch on"},
		{"literal is not true or 1", func() Flag {
			f := base()
			f.OnLiteral = "yes"
			return f
		}, "onLiteral \"yes\" must be true or 1"},
		{"purpose does not name the literal", func() Flag {
			f := base()
			f.Purpose = "Example switch."
			return f
		}, "the purpose must say that only the literal true turns it on"},
		{"purpose names a different literal", func() Flag {
			f := base()
			f.Purpose = "Example gate. Only the literal 1 turns it on."
			return f
		}, "the purpose must say that only the literal true turns it on"},
		{"default-on switch declares 1", func() Flag {
			f := base()
			f.Default = "true"
			f.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_ENABLED", Default: "1"}}
			f.OnDefaultReason = "Example."
			return f
		}, "the value \"1\" reads as on, but only the exact value true turns the switch on"},
		{"default-on switch default is on, unlike its literal", func() Flag {
			f := base()
			f.Default = "yes"
			f.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_ENABLED", Default: "yes"}}
			f.OnDefaultReason = "Example."
			return f
		}, "the value \"yes\" reads as on"},
		{"codeDefault reads as on under the canonical vocabulary", func() Flag {
			f := base()
			f.CodeDefault = "on"
			f.DriftReason = "Example."
			return f
		}, "the value \"on\" reads as on"},
		{"literal 1 gate declares true", func() Flag {
			f := base()
			f.OnLiteral = "1"
			f.Purpose = "Example gate. Only the literal 1 turns it on."
			f.Default = "true"
			f.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_ENABLED", Default: "true"}}
			f.OnDefaultReason = "Example."
			return f
		}, "the value \"true\" reads as on"},
		{"choice carrying the field", func() Flag {
			f := base()
			f.Kind = KindChoice
			f.Values = []string{"0", "1"}
			f.Default = "0"
			f.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_ENABLED", Default: "0"}}
			return f
		}, "only switch flags take onDefaultReason, driftReason, codeDefault, devOnly, onLiteral or onLiteralAnyCase"},
		{"text carrying the field", func() Flag {
			f := base()
			f.Kind = KindText
			f.Default = ""
			f.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_ENABLED", Default: ""}}
			return f
		}, "only switch flags take onDefaultReason, driftReason, codeDefault, devOnly, onLiteral or onLiteralAnyCase"},
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

// TestShippedOnLiteralMatchesThePurposeNames keeps the fields and the purposes in step for every shipped
// switch. A purpose that says the literal true turns the switch on must declare it: onLiteral true for an
// exact reader, or onLiteralAnyCase true when the purpose says in any case. A purpose that names the literal
// 1 must declare onLiteral 1, and no purpose names two literals. The word "literal" matches in any case, so
// "Literal true only" counts.
func TestShippedOnLiteralMatchesThePurposeNames(t *testing.T) {
	for _, f := range loadShipped(t).Flags() {
		if f.Kind != KindSwitch {
			continue
		}
		namesTrue := namesLiteral(f.Purpose, "true")
		namesOne := namesLiteral(f.Purpose, "1")
		// "in any case" counts only when it qualifies the literal true, as in "the literal true, in any case".
		// A purpose may say in any case about another value, such as the literal false on a default-on switch.
		anyCase := namesTrue && strings.Contains(stripTicks(f.Purpose), "in any case")
		if namesTrue && namesOne {
			t.Errorf("%s: purpose names both the literal true and the literal 1: %q", f.Name, f.Purpose)
		}
		switch {
		case anyCase:
			if f.OnLiteralAnyCase != "true" || f.OnLiteral != "" {
				t.Errorf("%s: purpose says the literal true is read in any case, but onLiteralAnyCase is %q and onLiteral is %q", f.Name, f.OnLiteralAnyCase, f.OnLiteral)
			}
		case f.OnLiteralAnyCase != "":
			t.Errorf("%s: onLiteralAnyCase is %q, but the purpose does not say in any case", f.Name, f.OnLiteralAnyCase)
		case namesTrue && f.OnLiteral != "true":
			t.Errorf("%s: purpose names the literal true but onLiteral is %q", f.Name, f.OnLiteral)
		case namesOne && f.OnLiteral != "1":
			t.Errorf("%s: purpose names the literal 1 but onLiteral is %q", f.Name, f.OnLiteral)
		}
	}
}

// TestShippedExactSwitchesReadOnlyTheirLiteral reads two shipped rows the finding names: ORG_AGENT_POLICY_API
// turns on only for the literal true, so a registry read of "1" or a padded "true" must stay off. The
// choice MODEL_ACTION_ADMISSION is matched as written, so a padded "table" is refused, not trimmed.
func TestShippedExactSwitchesReadOnlyTheirLiteral(t *testing.T) {
	reg := loadShipped(t)
	for raw, want := range map[string]bool{"true": true, " true ": false, "TRUE": false, "1": false, "on": false, "": false} {
		got, err := reg.Switch("ORG_AGENT_POLICY_API", lookupFrom(map[string]string{"ORG_AGENT_POLICY_API": raw}))
		if err != nil {
			t.Fatalf("ORG_AGENT_POLICY_API with %q: unexpected error %v", raw, err)
		}
		if got != want {
			t.Fatalf("ORG_AGENT_POLICY_API with %q = %t, want %t", raw, got, want)
		}
	}
	if _, err := reg.Choice("MODEL_ACTION_ADMISSION", lookupFrom(map[string]string{"MODEL_ACTION_ADMISSION": " table "})); !errors.Is(err, ErrUnknownChoice) {
		t.Fatalf("MODEL_ACTION_ADMISSION padded table: error = %v, want ErrUnknownChoice", err)
	}
	if got, err := reg.Choice("MODEL_ACTION_ADMISSION", lookupFrom(map[string]string{"MODEL_ACTION_ADMISSION": "table"})); err != nil || got != "table" {
		t.Fatalf("MODEL_ACTION_ADMISSION table = %q, %v; want table", got, err)
	}
}

// anyCaseExample is a switch whose reader trims surrounding space and folds ASCII case before it
// compares with the literal true, as the gateway's review run and teach mode readers do.
func anyCaseExample() Flag {
	f := exampleSwitch()
	f.Name = "EXAMPLE_ANY_CASE_ENABLED"
	f.Purpose = "Example any-case switch. Only the literal true, in any case, turns it on."
	f.OnLiteralAnyCase = "true"
	f.Declarations = []Declaration{{Service: "model-gateway", Env: "EXAMPLE_ANY_CASE_ENABLED", Default: "false"}}
	return f
}

// TestOnLiteralAnyCaseSwitchReadsTheTrimmedFoldedLiteral pins the typed read of an any-case switch to its
// reader: surrounding space and any casing of true turn it on, and 1, on, yes and a space inside the word do not.
func TestOnLiteralAnyCaseSwitchReadsTheTrimmedFoldedLiteral(t *testing.T) {
	reg, err := New(exampleFile(anyCaseExample()))
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	cases := []struct {
		value string
		set   bool
		want  bool
		why   string
	}{
		{"", false, false, "unset takes the off default"},
		{"", true, false, "an empty variable takes the off default"},
		{"true", true, true, "the literal turns it on"},
		{"TRUE", true, true, "any case turns it on"},
		{" True ", true, true, "surrounding space is trimmed"},
		{"\ttrue", true, true, "a tab is trimmed too"},
		{"true ", true, true, "trailing space is trimmed"},
		{"1", true, false, "1 is off"},
		{"yes", true, false, "yes is off"},
		{"on", true, false, "on is off"},
		{"enabled", true, false, "enabled is off"},
		{"tru e", true, false, "a space inside the word is not trimmed"},
		{"   ", true, false, "blank is off, and not the default"},
	}
	for _, tc := range cases {
		t.Run(tc.why, func(t *testing.T) {
			lookup := lookupFrom(nil)
			if tc.set {
				lookup = lookupFrom(map[string]string{"EXAMPLE_ANY_CASE_ENABLED": tc.value})
			}
			got, err := reg.Switch("EXAMPLE_ANY_CASE_ENABLED", lookup)
			if err != nil {
				t.Fatalf("Switch with %q: unexpected error %v", tc.value, err)
			}
			if got != tc.want {
				t.Fatalf("Switch with %q = %t, want %t", tc.value, got, tc.want)
			}
		})
	}
}

// TestShippedAnyCaseSwitchesArePinnedToTheGatewayNames pins the only two shipped any-case switches, the
// review run and teach mode. Their registry rows declare onLiteralAnyCase, and their typed reads fold case
// and trim, while 1 and on stay off. The gateway's DOCUMENTED_EXCEPTIONS names the same two switches.
func TestShippedAnyCaseSwitchesArePinnedToTheGatewayNames(t *testing.T) {
	reg := loadShipped(t)
	var declared []string
	for _, f := range reg.Flags() {
		if f.OnLiteralAnyCase != "" {
			declared = append(declared, f.Name)
		}
	}
	want := []string{"MODEL_GATEWAY_REVIEW_RUN_ENABLED", "MODEL_GATEWAY_TEACH_MODE_ENABLED"}
	if !slices.Equal(declared, want) {
		t.Fatalf("switches with onLiteralAnyCase = %v, want %v", declared, want)
	}
	for _, name := range want {
		f, _ := reg.Flag(name)
		if f.OnLiteral != "" || f.OnLiteralAnyCase != "true" {
			t.Errorf("%s: onLiteral %q and onLiteralAnyCase %q, want no onLiteral and onLiteralAnyCase true", name, f.OnLiteral, f.OnLiteralAnyCase)
		}
		for raw, wantOn := range map[string]bool{"true": true, "TRUE": true, " true ": true, "1": false, "on": false, "yes": false} {
			got, err := reg.Switch(name, lookupFrom(map[string]string{name: raw}))
			if err != nil {
				t.Fatalf("%s with %q: unexpected error %v", name, raw, err)
			}
			if got != wantOn {
				t.Errorf("%s with %q = %t, want %t", name, raw, got, wantOn)
			}
		}
	}
}

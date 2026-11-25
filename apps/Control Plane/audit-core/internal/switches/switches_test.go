package switches

import (
	"bytes"
	"errors"
	"strings"
	"testing"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
)

// capture points the global logger at a buffer for one test.
func capture(t *testing.T) *bytes.Buffer {
	t.Helper()
	var buf bytes.Buffer
	previous := log.Logger
	log.Logger = zerolog.New(&buf)
	t.Cleanup(func() { log.Logger = previous })
	return &buf
}

func env(values map[string]string) func(string) string {
	return func(name string) string { return values[name] }
}

func TestOnlyTheExactLiteralTurnsASwitchOn(t *testing.T) {
	for raw, want := range map[string]bool{
		"true": true,
		"":     false, "TRUE": false, "True": false, " true": false, "true ": false,
		"1": false, "on": false, "yes": false, "enabled": false,
		"0": false, "false": false, "off": false, "ture": false,
	} {
		if got := On(raw); got != want {
			t.Errorf("On(%q) = %v, want %v", raw, got, want)
		}
	}
}

func TestFormerSpellingsAreTheLenientOnWordsOnly(t *testing.T) {
	for _, raw := range []string{"1", " 1", "1 ", "on", "ON", "yes", "Yes", "enabled", "TRUE", "True", " true", "true "} {
		if !FormerSpelling(raw) {
			t.Errorf("FormerSpelling(%q) = false, want true", raw)
		}
	}
	for _, raw := range []string{"", "true", "0", "false", "off", "no", "ture", "y"} {
		if FormerSpelling(raw) {
			t.Errorf("FormerSpelling(%q) = true, want false", raw)
		}
	}
}

func TestMisreadIsEverythingThatReadsOffButIsNotAnOffWord(t *testing.T) {
	for _, raw := range []string{"1", "on", "yes", "True", " true", "ture", "y", "maybe"} {
		if !Misread(raw) {
			t.Errorf("Misread(%q) = false, want true", raw)
		}
	}
	for _, raw := range []string{"", "  ", "true", "false", "FALSE", "0", "off", "no", "disabled"} {
		if Misread(raw) {
			t.Errorf("Misread(%q) = true, want false", raw)
		}
	}
}

func TestCheckIsSilentForTheExactLiteralAndTheOffWords(t *testing.T) {
	buf := capture(t)
	list := []Switch{{Name: "AAA_ENABLED", Refuse: RefuseAlways}, {Name: "BBB_ENABLED", Refuse: RefuseInProduction}}
	values := map[string]string{"AAA_ENABLED": "true", "BBB_ENABLED": "false"}
	if err := Check(env(values), true, list); err != nil {
		t.Fatalf("exact and off values refused: %v", err)
	}
	if buf.Len() != 0 {
		t.Errorf("exact and off values logged: %s", buf.String())
	}
}

func TestCheckWarnsForEachMisreadSwitchAndNeverLogsItsValue(t *testing.T) {
	buf := capture(t)
	list := []Switch{{Name: "AAA_ENABLED"}, {Name: "BBB_ENABLED"}, {Name: "CCC_ENABLED"}}
	values := map[string]string{"AAA_ENABLED": "TRUE", "BBB_ENABLED": "yes", "CCC_ENABLED": "true"}
	if err := Check(env(values), false, list); err != nil {
		t.Fatalf("a warning-only switch refused boot: %v", err)
	}
	out := buf.String()
	for _, name := range []string{"AAA_ENABLED", "BBB_ENABLED"} {
		if !strings.Contains(out, name) {
			t.Errorf("no warning names %s: %s", name, out)
		}
	}
	if strings.Contains(out, "CCC_ENABLED") {
		t.Errorf("an exact value was reported: %s", out)
	}
	if strings.Contains(out, "TRUE") || strings.Contains(out, "yes") {
		t.Errorf("a warning repeats the value: %s", out)
	}
	if !strings.Contains(out, `"former_spelling":true`) {
		t.Errorf("a former spelling is not marked as one: %s", out)
	}
}

func TestCheckRefusesOnlyWhereThePolicyAsks(t *testing.T) {
	capture(t)
	cases := []struct {
		name       string
		refuse     Refusal
		production bool
		refused    bool
	}{
		{"warn only, production", WarnOnly, true, false},
		{"warn only, development", WarnOnly, false, false},
		{"production policy, production", RefuseInProduction, true, true},
		{"production policy, development", RefuseInProduction, false, false},
		{"always policy, development", RefuseAlways, false, true},
		{"always policy, production", RefuseAlways, true, true},
	}
	for _, tc := range cases {
		list := []Switch{{Name: "GUARD_ENABLED", Refuse: tc.refuse}}
		err := Check(env(map[string]string{"GUARD_ENABLED": "yes"}), tc.production, list)
		var typed *SwitchError
		if got := errors.As(err, &typed); got != tc.refused {
			t.Errorf("%s: refused=%v, want %v (err %v)", tc.name, got, tc.refused, err)
			continue
		}
		if tc.refused && (typed.Name != "GUARD_ENABLED" || err.Error() != "GUARD_ENABLED must be true or false") {
			t.Errorf("%s: typed error %q for %q", tc.name, err, typed.Name)
		}
	}
}

func TestCheckRefusesTheFirstRefusingSwitchInListOrder(t *testing.T) {
	capture(t)
	list := []Switch{
		{Name: "WARN_ONLY"},
		{Name: "FIRST_REFUSAL", Refuse: RefuseInProduction},
		{Name: "SECOND_REFUSAL", Refuse: RefuseAlways},
	}
	values := map[string]string{"WARN_ONLY": "1", "FIRST_REFUSAL": "on", "SECOND_REFUSAL": "yes"}
	var typed *SwitchError
	if err := Check(env(values), true, list); !errors.As(err, &typed) || typed.Name != "FIRST_REFUSAL" {
		t.Fatalf("refused %v, want FIRST_REFUSAL", err)
	}
}

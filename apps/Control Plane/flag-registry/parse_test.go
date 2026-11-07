package flagregistry

import (
	"errors"
	"testing"
)

func TestParseSwitchCanonicalVocabulary(t *testing.T) {
	cases := []struct {
		raw  string
		want bool
	}{
		{"1", true},
		{"true", true},
		{"TRUE", true},
		{" on ", true},
		{"yes", true},
		{"Enabled", true},
		{"0", false},
		{"false", false},
		{"off", false},
		{"no", false},
		{"disabled", false},
		{"", false},
		{"   ", false},
	}
	for _, tc := range cases {
		got, err := ParseSwitch(tc.raw)
		if err != nil {
			t.Errorf("ParseSwitch(%q): unexpected error: %v", tc.raw, err)
			continue
		}
		if got != tc.want {
			t.Errorf("ParseSwitch(%q) = %t, want %t", tc.raw, got, tc.want)
		}
	}
}

func TestParseSwitchRejectsUnknownValues(t *testing.T) {
	for _, raw := range []string{"2", "maybe", "t", "y", "enable", "nope", "true1", "onn"} {
		if _, err := ParseSwitch(raw); !errors.Is(err, ErrUnknownSwitchValue) {
			t.Errorf("ParseSwitch(%q) error = %v, want ErrUnknownSwitchValue", raw, err)
		}
	}
}

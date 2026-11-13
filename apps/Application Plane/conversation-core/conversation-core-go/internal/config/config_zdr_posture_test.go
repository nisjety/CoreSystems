package config

import "testing"

// CONVERSATION_ZDR_EFFECTIVE_POSTURE_ENABLED is a strict-literal switch: only
// "true" (surrounding whitespace trimmed) turns it on; every other value,
// including unset, leaves the raw-metadata reader in place.
func TestZDREffectivePostureReadStrictLiteral(t *testing.T) {
	cases := []struct {
		value string
		want  bool
	}{
		{"", false},
		{"false", false},
		{"TRUE", false},
		{"True", false},
		{"1", false},
		{"yes", false},
		{"on", false},
		{"true ", true},
		{" true", true},
		{"true", true},
	}
	for _, tc := range cases {
		t.Run("value="+tc.value, func(t *testing.T) {
			conversationEnvironment(t)
			t.Setenv("CONVERSATION_ZDR_EFFECTIVE_POSTURE_ENABLED", tc.value)
			cfg, err := Load()
			if err != nil {
				t.Fatalf("Load() error = %v", err)
			}
			if cfg.ZDREffectivePostureRead != tc.want {
				t.Fatalf("ZDREffectivePostureRead = %v for %q; want %v", cfg.ZDREffectivePostureRead, tc.value, tc.want)
			}
		})
	}
}

package config

import "testing"

// orgErasureNovuEnvironment is a configuration in which the provider-side erasure can reach Novu.
func orgErasureNovuEnvironment(t *testing.T) {
	t.Helper()
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_MODE", "novu")
	t.Setenv("NOVU_SECRET_KEY", "novu-fixture-key")
}

// Organization erasure is off unless NOTIFICATION_ORG_ERASURE_ENABLED is the exact literal true. A purge removes rows,
// so the switch must not turn on for a loose spelling.
func TestOrgErasureIsOffUnlessTheLiteralTrueIsSet(t *testing.T) {
	for value, want := range map[string]bool{
		"":      false,
		"false": false,
		"1":     false,
		"yes":   false,
		"TRUE":  false,
		" true": false,
	} {
		baseEnvironment(t)
		t.Setenv("NOTIFICATION_ORG_ERASURE_ENABLED", value)
		cfg, err := Load()
		if err != nil {
			t.Fatalf("Load() with %q error = %v", value, err)
		}
		if cfg.OrgErasureEnabled != want {
			t.Fatalf("NOTIFICATION_ORG_ERASURE_ENABLED=%q: OrgErasureEnabled = %v, want %v", value, cfg.OrgErasureEnabled, want)
		}
	}
}

// The literal true turns the erasure on in novu mode, where the provider-side erasure runs first.
func TestOrgErasureIsOnForTheLiteralTrueWithNovuMode(t *testing.T) {
	orgErasureNovuEnvironment(t)
	t.Setenv("NOTIFICATION_ORG_ERASURE_ENABLED", "true")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if !cfg.OrgErasureEnabled {
		t.Fatal("OrgErasureEnabled = false, want true for the literal true in novu mode")
	}
}

// Without the Novu provider there are no provider records to erase, so the switch is accepted and the erasure purges the
// local rows alone. A refusal here would stop notification-core at boot in the default disabled stack.
func TestLoadAcceptsOrgErasureInDisabledMode(t *testing.T) {
	baseEnvironment(t)
	t.Setenv("NOTIFICATION_DELIVERY_MODE", "disabled")
	t.Setenv("NOTIFICATION_ORG_ERASURE_ENABLED", "true")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v, want the switch accepted in disabled mode", err)
	}
	if !cfg.OrgErasureEnabled {
		t.Fatal("OrgErasureEnabled = false, want true for the literal true in disabled mode")
	}
}

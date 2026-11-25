package api

import "testing"

// F-f09-13 (owner decision D-f09-3): web.covers_browser is the org's choice whether its browser tools follow web.mode.
// Its default covers them, its opt-out is exactly false, and it is refused like any other value that is not a bool (O-19).

func TestOrgAgentPolicyCoversBrowserDefaultsOnAndTakesFalse(t *testing.T) {
	if got := orgAgentPolicyDefaults()["web.covers_browser"]; got != true {
		t.Fatalf("default web.covers_browser = %v, want true", got)
	}
	ceiling := loosestOrgAgentCeiling()
	settings, refusal := validateOrgAgentSettings(map[string]any{"web.covers_browser": false}, ceiling)
	if refusal != nil || settings["web.covers_browser"] != false {
		t.Fatalf("false refused or not stored: settings=%v refusal=%v", settings["web.covers_browser"], refusal)
	}
}

func TestOrgAgentPolicyCoversBrowserRefusesAValueThatIsNotABool(t *testing.T) {
	ceiling := loosestOrgAgentCeiling()
	for _, raw := range []any{"maybe", "off", "on", "false", "TRUE", float64(0), nil} {
		if _, refusal := validateOrgAgentSettings(map[string]any{"web.covers_browser": raw}, ceiling); refusal == nil || refusal.code != "invalid_value" {
			t.Fatalf("web.covers_browser = %v: refusal = %v, want invalid_value", raw, refusal)
		}
	}
}

func TestOrgAgentPolicyCoversBrowserIsTightenOnlyUnderACeiling(t *testing.T) {
	strict := loosestOrgAgentCeiling()
	strict["web.covers_browser"] = "true"
	if _, refusal := validateOrgAgentSettings(map[string]any{"web.covers_browser": false}, strict); refusal == nil || refusal.code != "exceeds_platform_ceiling" {
		t.Fatalf("false under a true ceiling: refusal = %v, want exceeds_platform_ceiling", refusal)
	}
	settings, refusal := validateOrgAgentSettings(map[string]any{"web.covers_browser": true}, strict)
	if refusal != nil || settings["web.covers_browser"] != true {
		t.Fatalf("true under a true ceiling: settings=%v refusal=%v", settings["web.covers_browser"], refusal)
	}
}

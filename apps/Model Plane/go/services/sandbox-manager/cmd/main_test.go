package main

import "testing"

func TestEphemeralDevelopmentEnabledRequiresExactOptIn(t *testing.T) {
	for _, value := range []string{"", "1", "TRUE", "yes", "false"} {
		if ephemeralDevelopmentEnabled(value) {
			t.Fatalf("ephemeral store unexpectedly enabled for %q", value)
		}
	}
	if !ephemeralDevelopmentEnabled("true") {
		t.Fatal("exact development opt-in must enable the test-only store")
	}
}

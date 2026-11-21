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

func TestOverlayTombstonesEnabledRequiresTheStrictLiteral(t *testing.T) {
	for _, value := range []string{"", "1", "TRUE", "True", "yes", "false", " true"} {
		if overlayTombstonesEnabled(value) {
			t.Fatalf("overlay tombstones unexpectedly enabled for %q", value)
		}
	}
	if !overlayTombstonesEnabled("true") {
		t.Fatal(`the literal "true" must enable overlay tombstones`)
	}
}

func TestServiceThreadEraseEnabledRequiresTheStrictLiteral(t *testing.T) {
	for _, value := range []string{"", "false", "TRUE", "True", "1", "yes", " true", "true "} {
		if serviceThreadEraseEnabled(value) {
			t.Fatalf("%q must not enable the service thread erase", value)
		}
	}
	if !serviceThreadEraseEnabled("true") {
		t.Fatal(`the literal "true" must enable the service thread erase`)
	}
}

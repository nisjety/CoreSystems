package main

import "testing"

func TestUnsignedGDPRConsumerRequiresTwoExplicitDevGates(t *testing.T) {
	t.Setenv("ALLOW_UNVERIFIED_LEGACY_EVENTS", "")
	t.Setenv("ALLOW_INSECURE_DEV_DEFAULTS", "")
	if unverifiedLegacyEventsEnabled() {
		t.Fatal("unsigned consumer must default off")
	}
	t.Setenv("ALLOW_UNVERIFIED_LEGACY_EVENTS", "1")
	if unverifiedLegacyEventsEnabled() {
		t.Fatal("one gate must not enable unsigned consumer")
	}
	t.Setenv("ALLOW_INSECURE_DEV_DEFAULTS", "1")
	if !unverifiedLegacyEventsEnabled() {
		t.Fatal("both explicit development gates should enable the legacy consumer")
	}
}

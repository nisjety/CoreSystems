package main

import "testing"

func TestUnsignedAsyncEventsRequireThreeIsolatedDevGates(t *testing.T) {
	t.Setenv("ALLOW_UNVERIFIED_LEGACY_EVENTS", "")
	t.Setenv("ALLOW_INSECURE_DEV_DEFAULTS", "")
	t.Setenv("ISOLATED_E2E", "")
	if unverifiedLegacyEventsEnabled() {
		t.Fatal("unsigned consumer must default off")
	}
	t.Setenv("ALLOW_UNVERIFIED_LEGACY_EVENTS", "1")
	if unverifiedLegacyEventsEnabled() {
		t.Fatal("one gate must not enable unsigned consumer")
	}
	t.Setenv("ALLOW_INSECURE_DEV_DEFAULTS", "1")
	if unverifiedLegacyEventsEnabled() {
		t.Fatal("production posture must not enable unsigned events")
	}
	t.Setenv("ISOLATED_E2E", "1")
	if !unverifiedLegacyEventsEnabled() {
		t.Fatal("all three explicit isolated development gates should enable legacy events")
	}
}

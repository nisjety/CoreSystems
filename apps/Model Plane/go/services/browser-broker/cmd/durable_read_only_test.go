package main

import (
	"context"
	"testing"
)

// BROWSER_BROKER_DURABLE_READ_ONLY_ENABLED reads the exact literal "true" only:
// no trimming, no case folding, and no "1", "yes" or "on".
func TestDurableReadOnlySwitchIsTheExactLiteralTrue(t *testing.T) {
	for _, value := range []string{"", "0", "false", "1", "yes", "on", "TRUE", " true "} {
		options, err := serverOptionsFromEnv(envOf(map[string]string{envDurableReadOnly: value}))
		if err != nil {
			t.Fatalf("%q: serverOptionsFromEnv: %v", value, err)
		}
		if options.DurableReadOnly {
			t.Fatalf("%q: durable read-only enabled; only the exact literal true enables it", value)
		}
	}
	options, err := serverOptionsFromEnv(envOf(map[string]string{envDurableReadOnly: "true"}))
	if err != nil {
		t.Fatalf("serverOptionsFromEnv: %v", err)
	}
	if !options.DurableReadOnly {
		t.Fatal(`BROWSER_BROKER_DURABLE_READ_ONLY_ENABLED="true" must enable durable read-only`)
	}
}

// With the switch off there is nothing to confirm: it stays off, store or not.
func TestDurableReadOnlyReady_OffStaysOff(t *testing.T) {
	if durableReadOnlyReady(context.Background(), nil, false) {
		t.Fatal("switch off must stay off")
	}
}

// With no database (ephemeral development) the in-memory store keeps the read
// level itself, so the switch is taken as set. The Postgres column check is the
// grant package's integration test TestPg_ReadOnlyColumnPresentFollowsTheMigration.
func TestDurableReadOnlyReady_EphemeralStoreTakesTheSwitchAsSet(t *testing.T) {
	if !durableReadOnlyReady(context.Background(), nil, true) {
		t.Fatal("the in-memory store keeps the read level, so the switch must stay on")
	}
}

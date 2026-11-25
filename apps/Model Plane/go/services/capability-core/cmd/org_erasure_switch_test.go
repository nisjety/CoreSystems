package main

import (
	"testing"

	"github.com/nats-io/nats.go"
)

// envLookup returns a lookup that answers only the named key.
func envLookup(key, value string) func(string) string {
	return func(name string) string {
		if name == key {
			return value
		}
		return ""
	}
}

// TestOrgErasureIsBoundOnlyByTheLiteralTrue pins the switch: only the literal true enables the consumer.
func TestOrgErasureIsBoundOnlyByTheLiteralTrue(t *testing.T) {
	for _, raw := range []string{"", "false", "0", "1", "yes", "on", "TRUE", " true "} {
		if orgErasureEnabled(envLookup(orgErasureEnabledEnv, raw)) {
			t.Fatalf("%s=%q enabled the org erasure consumer; only the literal true may", orgErasureEnabledEnv, raw)
		}
	}
	if !orgErasureEnabled(envLookup(orgErasureEnabledEnv, "true")) {
		t.Fatalf("%s=true must enable the org erasure consumer", orgErasureEnabledEnv)
	}
}

// TestOrgErasureStartsNothingWhileTheSwitchIsOff pins switch-off behaviour: with the switch off, no broker URL is read
// and nothing is started, even when the URL is configured.
func TestOrgErasureStartsNothingWhileTheSwitchIsOff(t *testing.T) {
	lookup := func(name string) string {
		switch name {
		case orgErasureEnabledEnv:
			return "false"
		case orgErasureNATSURLEnv:
			return "nats://control-shared-nats:4222"
		}
		return ""
	}
	if url, ok := orgErasureTarget(lookup); ok || url != "" {
		t.Fatalf("switch off: orgErasureTarget = %q, %v; nothing may start", url, ok)
	}
}

// TestOrgErasureNeedsABrokerURLWhenOn pins that an enabled consumer with no shared-broker URL starts nothing, and that
// a configured URL is used trimmed.
func TestOrgErasureNeedsABrokerURLWhenOn(t *testing.T) {
	on := func(url string) func(string) string {
		return func(name string) string {
			switch name {
			case orgErasureEnabledEnv:
				return "true"
			case orgErasureNATSURLEnv:
				return url
			}
			return ""
		}
	}
	for _, url := range []string{"", "   "} {
		if got, ok := orgErasureTarget(on(url)); ok {
			t.Fatalf("switch on with URL %q: started against %q; it must start nothing", url, got)
		}
	}
	if got, ok := orgErasureTarget(on("  nats://control-shared-nats:4222  ")); !ok || got != "nats://control-shared-nats:4222" {
		t.Fatalf("switch on: orgErasureTarget = %q, %v; want the trimmed shared-broker URL", got, ok)
	}
}

// TestOrgErasureConnectionHasItsOwnInboxAndIdentity pins that the consumer's shared-broker connection keeps its own inbox
// namespace, separate from the plane-local connection, retries the first connect, and carries the narrow identity only
// when one is configured.
func TestOrgErasureConnectionHasItsOwnInboxAndIdentity(t *testing.T) {
	apply := func(options []nats.Option) nats.Options {
		t.Helper()
		resolved := nats.GetDefaultOptions()
		for _, option := range options {
			if err := option(&resolved); err != nil {
				t.Fatalf("apply option: %v", err)
			}
		}
		return resolved
	}

	anonymous := apply(orgErasureNATSOptions(func(string) string { return "" }))
	if anonymous.InboxPrefix != orgErasureInboxPrefix {
		t.Fatalf("inbox prefix = %q, want %q", anonymous.InboxPrefix, orgErasureInboxPrefix)
	}
	if anonymous.InboxPrefix == natsInboxPrefix {
		t.Fatal("the shared-broker connection must not share the plane-local inbox namespace")
	}
	if !anonymous.RetryOnFailedConnect {
		t.Fatal("the shared-broker connection must retry its first connect")
	}
	if anonymous.User != "" || anonymous.Password != "" {
		t.Fatalf("no identity configured, but the connection carries user=%q", anonymous.User)
	}

	identified := apply(orgErasureNATSOptions(func(name string) string {
		switch name {
		case orgErasureNATSUserEnv:
			return " capability-core-gdpr "
		case orgErasureNATSPasswordEnv:
			return "fixture-password-aaaaaaaaaaaaaaaa"
		}
		return ""
	}))
	if identified.User != "capability-core-gdpr" || identified.Password != "fixture-password-aaaaaaaaaaaaaaaa" {
		t.Fatalf("identity = %q / %q; want the configured capability-core-gdpr identity", identified.User, identified.Password)
	}
}

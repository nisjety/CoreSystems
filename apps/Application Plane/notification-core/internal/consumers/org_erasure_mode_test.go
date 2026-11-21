package consumers

import (
	"context"
	"errors"
	"strings"
	"testing"
)

// The disabled Novu adapter answers every provider call with an error, as a provider step that cannot reach Novu would. A
// disabled deployment holds no provider record, so its wiring runs no provider step: the message acks and every local
// store runs.
func TestOrgErasure_DisabledModeWiring_RunsTheLocalPurgeWithoutAProviderStep(t *testing.T) {
	logged := captureLog(t)
	order := &orderLog{}
	provider := &recordingPurger{name: "provider", order: order, err: errors.New("delivery disabled")}
	rows := &recordingPurger{name: "rows", order: order, counts: map[string]int64{"notification_requests": 2}}
	pushes := &recordingPurger{name: "push", order: order, counts: map[string]int64{"notification_web_push_subscriptions": 1}}
	s := NewOrgErasureSubscriberForMode(nil, "disabled", provider, rows, pushes)

	if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureAck {
		t.Fatalf("outcome = %v, want orgErasureAck: no provider step may run in disabled mode", got)
	}
	if got, want := order.snapshot(), []string{"rows", "push"}; strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("purge order = %v, want %v with no provider step", got, want)
	}
	if !strings.Contains(logged.String(), "provider-side erasure is off because the delivery mode is not novu") {
		t.Fatalf("log = %q, want the one-line note that the provider side is off", logged.String())
	}
}

// The novu wiring keeps the provider step, and it runs before every local store.
func TestOrgErasure_NovuModeWiring_RunsTheProviderStepFirst(t *testing.T) {
	logged := captureLog(t)
	order := &orderLog{}
	provider := &recordingPurger{name: "provider", order: order, counts: map[string]int64{"novu_subscribers": 1}}
	rows := &recordingPurger{name: "rows", order: order, counts: map[string]int64{"notification_requests": 2}}
	pushes := &recordingPurger{name: "push", order: order, counts: map[string]int64{"notification_web_push_subscriptions": 1}}
	s := NewOrgErasureSubscriberForMode(nil, "novu", provider, rows, pushes)

	if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureAck {
		t.Fatalf("outcome = %v, want orgErasureAck", got)
	}
	if got, want := order.snapshot(), []string{"provider", "rows", "push"}; strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("purge order = %v, want %v: the provider step must run first in novu mode", got, want)
	}
	if strings.Contains(logged.String(), "provider-side erasure is off") {
		t.Fatalf("log = %q: the off line must not appear in novu mode", logged.String())
	}
}

// In novu mode a failed provider step keeps every local row in place. This pins the gate in the wiring, not only the
// order of the steps.
func TestOrgErasure_NovuModeWiring_ProviderFailureKeepsEveryLocalRow(t *testing.T) {
	captureLog(t)
	order := &orderLog{}
	provider := &recordingPurger{name: "provider", order: order, err: errors.New("novu unavailable")}
	rows := &recordingPurger{name: "rows", order: order, counts: map[string]int64{"notification_requests": 2}}
	s := NewOrgErasureSubscriberForMode(nil, "novu", provider, rows)

	if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureRetry {
		t.Fatalf("outcome = %v, want orgErasureRetry for a provider-side failure in novu mode", got)
	}
	if got := order.snapshot(); len(got) != 1 || got[0] != "provider" {
		t.Fatalf("purge order = %v, want only the provider step: a local store ran while the provider step failed", got)
	}
}

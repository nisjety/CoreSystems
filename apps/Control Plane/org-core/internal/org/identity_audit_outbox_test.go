package org

import (
	"context"
	"strings"
	"testing"
	"time"
)

// WS05-S6/S7: the identity audit outbox payload. Pure; the insert itself is covered in domain_postgres_test.go.

func TestIdentityAuditPayloadShape(t *testing.T) {
	at := time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)
	change := identityAuditChange{
		Event: IdentityEventDomainVerified, OrgID: "org-1", ActorID: "user-1", ActorRole: "admin",
		Resource: "example.com", Details: map[string]any{"domain": "example.com", "method": "dns_txt"},
	}
	eventID, subject, payload := identityAuditPayload(change, at)

	if subject != "verevon.audit.v2.control.org-core.org_domain_verified" {
		t.Fatalf("subject = %q", subject)
	}
	// audit-core accepts only six-token subjects: verevon.audit.v2.<plane>.<producer>.<event>.
	if got := len(strings.Split(subject, ".")); got != 6 {
		t.Fatalf("subject has %d tokens, want 6", got)
	}
	if !strings.HasPrefix(eventID, "identity:org-core:") || len(eventID) > 128 {
		t.Fatalf("event id %q", eventID)
	}
	want := map[string]any{
		"event_id": eventID, "org_id": "org-1", "user_id": "user-1", "actor_role": "admin",
		"plane": "control", "producer": "org-core", "event": IdentityEventDomainVerified,
		"subject": "domain:example.com", "resource_id": "example.com", "outcome": "ok",
	}
	for key, value := range want {
		if payload[key] != value {
			t.Fatalf("payload[%s] = %v, want %v", key, payload[key], value)
		}
	}
	if payload["occurred_at"] != "2026-10-01T12:00:00Z" {
		t.Fatalf("occurred_at = %v", payload["occurred_at"])
	}
	// Details are copied, not aliased.
	change.Details["domain"] = "mutated.example"
	if payload["details"].(map[string]any)["domain"] != "example.com" {
		t.Fatal("payload details alias the caller's map")
	}
}

func TestIdentityAuditEventIDIsDeterministicAndDistinct(t *testing.T) {
	at := time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)
	base := identityAuditChange{Event: IdentityEventDomainAdded, OrgID: "org-1", ActorID: "u", ActorRole: "admin", Resource: "a.example.com"}
	a1, _, _ := identityAuditPayload(base, at)
	a2, _, _ := identityAuditPayload(base, at)
	if a1 != a2 {
		t.Fatal("the same change at the same instant must give the same id (outbox is keyed on it)")
	}
	otherDomain := base
	otherDomain.Resource = "b.example.com"
	otherOrg := base
	otherOrg.OrgID = "org-2"
	otherEvent := base
	otherEvent.Event = IdentityEventDomainRevoked
	for name, change := range map[string]identityAuditChange{"domain": otherDomain, "org": otherOrg, "event": otherEvent} {
		id, _, _ := identityAuditPayload(change, at)
		if id == a1 {
			t.Fatalf("a different %s must give a different id", name)
		}
	}
	if later, _, _ := identityAuditPayload(base, at.Add(time.Nanosecond)); later == a1 {
		t.Fatal("a later instant must give a different id")
	}
	long := base
	long.Resource = strings.Repeat("a", 60) + "." + strings.Repeat("b", 60) + ".example.com"
	if id, _, _ := identityAuditPayload(long, at); len(id) > 128 {
		t.Fatalf("a long domain must stay inside the 128 byte column, id is %d", len(id))
	}
}

func TestIdentityAuditResourceKind(t *testing.T) {
	cases := map[string]string{
		IdentityEventDomainAdded:      "domain",
		IdentityEventDomainVerified:   "domain",
		IdentityEventDomainRevoked:    "domain",
		IdentityEventSSOPolicyUpdated: "sso_policy",
		IdentityEventSSOProviderAdded: "sso_provider",
		IdentityEventSSOProviderGone:  "sso_provider",
	}
	for event, want := range cases {
		if got := identityAuditResourceKind(event); got != want {
			t.Fatalf("kind(%s) = %q, want %q", event, got, want)
		}
		if _, known := identityAuditEvents[event]; !known {
			t.Fatalf("%s is not in the known event set", event)
		}
	}
}

func TestEnqueueIdentityAuditRefusesBeforeTouchingTheTransaction(t *testing.T) {
	ctx := context.Background()
	good := identityAuditChange{Event: IdentityEventDomainAdded, OrgID: "org-1", ActorID: "u", ActorRole: "admin", Resource: "a.example.com"}

	unknown := good
	unknown.Event = "org.domain.added" // the plan's dotted name would be rejected by audit-core
	noOrg := good
	noOrg.OrgID = " "
	noActor := good
	noActor.ActorID = ""
	noRole := good
	noRole.ActorRole = "\t"
	for name, change := range map[string]identityAuditChange{"unknown event": unknown, "no org": noOrg, "no actor": noActor, "no role": noRole} {
		// A nil transaction proves the guard fires before any SQL.
		if err := enqueueIdentityAudit(ctx, nil, change); err == nil {
			t.Fatalf("%s: expected a refusal", name)
		}
	}
}

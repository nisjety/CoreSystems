package events

import (
	"fmt"
	"strings"
	"testing"
)

func TestDecodeUsageRequiresBoundedProducerStableEventID(t *testing.T) {
	valid := `{"event_id":"usage:model:req-01","occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","plane":"model","producer":"session-core","op":"tokens"}`
	decoded, err := DecodeUsage([]byte(valid))
	if err != nil {
		t.Fatal(err)
	}
	if decoded.EventID != "usage:model:req-01" {
		t.Fatalf("event_id = %q", decoded.EventID)
	}

	for _, eventID := range []string{"", "usage id", strings.Repeat("x", 129), "/starts-with-separator"} {
		t.Run(fmt.Sprintf("invalid-%d", len(eventID)), func(t *testing.T) {
			payload := fmt.Sprintf(`{"event_id":%q,"occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","plane":"model","producer":"session-core","op":"tokens"}`, eventID)
			if _, err := DecodeUsage([]byte(payload)); err == nil {
				t.Fatalf("invalid event_id %q was accepted", eventID)
			}
		})
	}
}

func TestDecodeUsageRejectsMalformedAndIncompleteEvents(t *testing.T) {
	t.Parallel()

	for name, payload := range map[string]string{
		"malformed-json":   `{`,
		"missing-org":      `{"event_id":"usage:model:req-01","occurred_at":"2026-07-15T12:00:00Z","plane":"model","producer":"session-core","op":"tokens"}`,
		"missing-plane":    `{"event_id":"usage:model:req-01","occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","producer":"session-core","op":"tokens"}`,
		"missing-producer": `{"event_id":"usage:model:req-01","occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","plane":"model","op":"tokens"}`,
		"missing-op":       `{"event_id":"usage:model:req-01","occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","plane":"model","producer":"session-core"}`,
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			if _, err := DecodeUsage([]byte(payload)); err == nil {
				t.Fatal("incomplete usage event was accepted")
			}
		})
	}
}

func TestDecodeUsageRejectsMissingProducerOccurrenceTime(t *testing.T) {
	t.Parallel()

	if _, err := DecodeUsage([]byte(`{"event_id":"usage:model:req-01","org_id":"org-1","plane":"model","producer":"session-core","op":"tokens"}`)); err == nil {
		t.Fatal("missing producer occurred_at was replaced by broker delivery time")
	}
}

func TestDecodeAuditRequiresStableProducerIdentityAndOccurrenceTime(t *testing.T) {
	t.Parallel()

	for name, payload := range map[string]string{
		"malformed-json":   `{`,
		"missing-event-id": `{"occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","plane":"control","producer":"auth-core","event":"signed_in"}`,
		"missing-time":     `{"event_id":"audit:auth:session-1","org_id":"org-1","plane":"control","producer":"auth-core","event":"signed_in"}`,
		"missing-org":      `{"event_id":"audit:auth:session-1","occurred_at":"2026-07-15T12:00:00Z","plane":"control","producer":"auth-core","event":"signed_in"}`,
		"missing-plane":    `{"event_id":"audit:auth:session-1","occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","producer":"auth-core","event":"signed_in"}`,
		"missing-producer": `{"event_id":"audit:auth:session-1","occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","plane":"control","event":"signed_in"}`,
		"missing-event":    `{"event_id":"audit:auth:session-1","occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","plane":"control","producer":"auth-core"}`,
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			if _, err := DecodeAudit([]byte(payload)); err == nil {
				t.Fatal("incomplete audit event was accepted")
			}
		})
	}

	event, err := DecodeAudit([]byte(`{"event_id":"audit:auth:session-1","occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","plane":"control","producer":"auth-core","event":"signed_in"}`))
	if err != nil {
		t.Fatal(err)
	}
	if event.Outcome != "ok" {
		t.Fatalf("default outcome = %q; want ok", event.Outcome)
	}
	if event.OccurredAt.IsZero() {
		t.Fatal("producer occurred_at was not preserved")
	}
	if got := errMissingField("org_id").Error(); got != "missing required field: org_id" {
		t.Fatalf("missing field error = %q", got)
	}
}

func TestDecodeAuditRejectsInvalidStableIdentity(t *testing.T) {
	t.Parallel()

	for _, eventID := range []string{"audit id", strings.Repeat("x", 129), "/starts-with-separator"} {
		payload := fmt.Sprintf(`{"event_id":%q,"occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","plane":"control","producer":"auth-core","event":"signed_in"}`, eventID)
		if _, err := DecodeAudit([]byte(payload)); err == nil {
			t.Fatalf("invalid event_id %q was accepted", eventID)
		}
	}
	for _, producer := range []string{"Auth Core", "auth.core", "-auth"} {
		payload := fmt.Sprintf(`{"event_id":"audit:auth:session-1","occurred_at":"2026-07-15T12:00:00Z","org_id":"org-1","plane":"control","producer":%q,"event":"signed_in"}`, producer)
		if _, err := DecodeAudit([]byte(payload)); err == nil {
			t.Fatalf("invalid producer %q was accepted", producer)
		}
	}
}

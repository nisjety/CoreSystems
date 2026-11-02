package spaces

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

func samplePolicyEvent() policyEvent {
	return policyEvent{
		OrgID: "org-1", Source: PolicyEventSourceOrgAdmin,
		Actor:      EffectPolicyActor{Type: "user", ID: "user-9", OrgRole: "admin"},
		ChangeKind: PolicyChangePresetApplied, PresetID: "collab",
		Before:         map[string]bool{"thread_create": false, "thread_read": false},
		After:          map[string]bool{"thread_create": true, "thread_read": true},
		Changes:        []EntitlementChange{{Key: "thread_create", From: false, To: true}, {Key: "thread_read", From: false, To: true}},
		PrivacyChanged: false, PolicyRevAfter: 4, EntRevBefore: 6, EntRevAfter: 7, SpacesFenced: 3,
	}
}

func TestBuildPolicyAuditPayloadCarriesOnlyBooleansAndKeyNames(t *testing.T) {
	now := time.Date(2026, 10, 27, 12, 0, 0, 0, time.UTC)
	raw, err := buildPolicyAuditPayload(42, samplePolicyEvent(), now)
	if err != nil {
		t.Fatalf("buildPolicyAuditPayload: %v", err)
	}
	var payload map[string]any
	if err := json.Unmarshal(raw, &payload); err != nil {
		t.Fatalf("payload is not JSON: %v", err)
	}
	if payload["event_id"] != "space-effect-policy:42" {
		t.Fatalf("event_id = %v, want space-effect-policy:<id>", payload["event_id"])
	}
	for field, want := range map[string]string{
		"org_id": "org-1", "user_id": "user-9", "actor_role": "admin", "plane": "control",
		"producer": "user-core", "event": "space_effect_policy_updated", "subject": "org:org-1",
		"resource_id": "org-1", "outcome": "ok",
	} {
		if payload[field] != want {
			t.Errorf("%s = %v, want %q", field, payload[field], want)
		}
	}
	if payload["occurred_at"] != "2026-10-27T12:00:00Z" {
		t.Errorf("occurred_at = %v", payload["occurred_at"])
	}

	details, ok := payload["details"].(map[string]any)
	if !ok {
		t.Fatalf("details missing: %v", payload)
	}
	if details["preset_id"] != "collab" || details["change_kind"] != PolicyChangePresetApplied ||
		details["source"] != PolicyEventSourceOrgAdmin || details["spaces_fenced"] != float64(3) ||
		details["entitlement_revision_before"] != float64(6) || details["entitlement_revision_after"] != float64(7) {
		t.Fatalf("details drifted: %v", details)
	}
	// Every scalar in details is a bool, a number or a known short token; the
	// changed list is key + bool + bool and nothing else.
	changed, ok := details["changed"].([]any)
	if !ok || len(changed) != 2 {
		t.Fatalf("changed = %v", details["changed"])
	}
	for _, entry := range changed {
		row := entry.(map[string]any)
		if len(row) != 3 {
			t.Fatalf("a changed row carries more than key/from/to: %v", row)
		}
		if _, ok := EntitlementSpecByKey(row["key"].(string)); !ok {
			t.Errorf("changed row names a key outside the catalogue: %v", row)
		}
		if _, ok := row["from"].(bool); !ok {
			t.Errorf("from is not a boolean: %v", row)
		}
		if _, ok := row["to"].(bool); !ok {
			t.Errorf("to is not a boolean: %v", row)
		}
	}
	allowed := map[string]bool{
		"source": true, "change_kind": true, "changed": true, "privacy_changed": true,
		"entitlement_revision_before": true, "entitlement_revision_after": true,
		"spaces_fenced": true, "preset_id": true,
	}
	for key := range details {
		if !allowed[key] {
			t.Errorf("details carries an unexpected key %q", key)
		}
	}
}

func TestBuildPolicyAuditPayloadOmitsPresetAndMapsServiceActor(t *testing.T) {
	event := samplePolicyEvent()
	event.PresetID = ""
	event.ChangeKind = PolicyChangeOperatorUpdated
	event.Source = PolicyEventSourceOperator
	event.Actor = EffectPolicyActor{Type: "service", ID: "control-space-policy"}
	raw, err := buildPolicyAuditPayload(7, event, time.Date(2026, 10, 27, 12, 0, 0, 0, time.UTC))
	if err != nil {
		t.Fatalf("buildPolicyAuditPayload: %v", err)
	}
	var payload struct {
		EventID   string         `json:"event_id"`
		UserID    string         `json:"user_id"`
		ActorRole string         `json:"actor_role"`
		Details   map[string]any `json:"details"`
	}
	if err := json.Unmarshal(raw, &payload); err != nil {
		t.Fatal(err)
	}
	if payload.EventID != "space-effect-policy:7" || payload.UserID != "service:control-space-policy" || payload.ActorRole != "operator" {
		t.Fatalf("service actor rendered as %+v", payload)
	}
	if _, ok := payload.Details["preset_id"]; ok {
		t.Fatal("an empty preset id must be omitted from details")
	}
}

func TestPolicyAuditPayloadNeverCarriesContentFields(t *testing.T) {
	raw, err := buildPolicyAuditPayload(1, samplePolicyEvent(), time.Now())
	if err != nil {
		t.Fatal(err)
	}
	// Zero Data Retention needs no special handling for this event precisely
	// because it never carries free text, thread content or tokens.
	text := string(raw)
	for _, forbidden := range []string{"content", "token", "secret", "password", "message", "email", "reason"} {
		if strings.Contains(strings.ToLower(text), forbidden) {
			t.Errorf("audit payload mentions %q: %s", forbidden, text)
		}
	}
}

func TestEffectPolicyActorValidateAndAuditIdentity(t *testing.T) {
	for _, actor := range []EffectPolicyActor{
		{Type: "", ID: "x"}, {Type: "robot", ID: "x"}, {Type: "user", ID: "  "}, {Type: "service", ID: ""},
	} {
		if err := actor.Validate(); err == nil {
			t.Errorf("actor %+v passed validation", actor)
		}
	}
	for _, actor := range []EffectPolicyActor{{Type: "user", ID: "u", OrgRole: "owner"}, {Type: "service", ID: "s"}} {
		if err := actor.Validate(); err != nil {
			t.Errorf("actor %+v refused: %v", actor, err)
		}
	}
	if (EffectPolicyActor{Type: "user", ID: "u1"}).auditUserID() != "u1" {
		t.Error("user actor id must be rendered as is")
	}
	if (EffectPolicyActor{Type: "service", ID: "s1"}).auditUserID() != "service:s1" {
		t.Error("service actor id must be prefixed service:")
	}
}

// An unwired outbox fails closed before the transaction is touched, so no
// policy change can land without its audit row.
func TestInsertPolicyEventTxFailsClosedWithoutOutbox(t *testing.T) {
	_, err := insertPolicyEventTx(context.Background(), nil, nil, samplePolicyEvent(), time.Now())
	if err == nil || !strings.Contains(err.Error(), "outbox is not wired") {
		t.Fatalf("unwired outbox: err = %v", err)
	}
}

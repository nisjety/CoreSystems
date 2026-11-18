package spaces

import (
	"encoding/json"
	"sort"
	"testing"
	"time"
)

// W3-DEC-ORGAGENT (D19): pure-function tests of the org-admin agent lifecycle
// (agent_org_admin.go). No database.

// TestOrgAgentNextLifecycle: suspend and reactivate from each valid starting
// state, retire from active and from suspended, retiring an already-retired
// agent is a no-op, and suspending or reactivating a retired agent is refused.
func TestOrgAgentNextLifecycle(t *testing.T) {
	cases := []struct {
		name        string
		action      string
		current     string
		wantNext    string
		wantChanged bool
		wantErr     bool
	}{
		{"suspend active", OrgAgentActionSuspend, AgentLifecycleActive, AgentLifecycleSuspended, true, false},
		{"suspend already suspended is a no-op", OrgAgentActionSuspend, AgentLifecycleSuspended, AgentLifecycleSuspended, false, false},
		{"reactivate suspended", OrgAgentActionReactivate, AgentLifecycleSuspended, AgentLifecycleActive, true, false},
		{"reactivate already active is a no-op", OrgAgentActionReactivate, AgentLifecycleActive, AgentLifecycleActive, false, false},
		{"retire active", OrgAgentActionRetire, AgentLifecycleActive, AgentLifecycleRetired, true, false},
		{"retire suspended", OrgAgentActionRetire, AgentLifecycleSuspended, AgentLifecycleRetired, true, false},
		{"retire an already-retired agent changes nothing", OrgAgentActionRetire, AgentLifecycleRetired, AgentLifecycleRetired, false, false},
		{"suspend a retired agent is refused", OrgAgentActionSuspend, AgentLifecycleRetired, "", false, true},
		{"reactivate a retired agent is refused", OrgAgentActionReactivate, AgentLifecycleRetired, "", false, true},
	}
	for _, tc := range cases {
		next, changed, err := OrgAgentNextLifecycle(tc.action, tc.current)
		if tc.wantErr {
			roomErr, ok := err.(*RoomError)
			if !ok || roomErr == nil {
				t.Errorf("%s: want a *RoomError, got %v", tc.name, err)
				continue
			}
			if roomErr.Status != 409 || roomErr.Code != CodeAgentLifecycleInvalid {
				t.Errorf("%s: want 409 %s, got %d %s", tc.name, CodeAgentLifecycleInvalid, roomErr.Status, roomErr.Code)
			}
			if roomErr.Details["lifecycle"] != tc.current || roomErr.Details["action"] != tc.action {
				t.Errorf("%s: details = %v, want lifecycle=%s action=%s", tc.name, roomErr.Details, tc.current, tc.action)
			}
			continue
		}
		if err != nil {
			t.Errorf("%s: unexpected error %v", tc.name, err)
			continue
		}
		if next != tc.wantNext || changed != tc.wantChanged {
			t.Errorf("%s: next=%q changed=%v, want next=%q changed=%v", tc.name, next, changed, tc.wantNext, tc.wantChanged)
		}
	}
}

// TestBuildOrgAgentAuditPayloadIsMinimal: the org-admin audit payload carries
// only ids, states, reasons and counts -- never any other field (no agent
// name, no instructions, no content).
func TestBuildOrgAgentAuditPayloadIsMinimal(t *testing.T) {
	facts := orgAgentAuditFacts{
		OrgID: "org-1", ActorID: "user-1", ActorRole: "admin", AgentID: "agent-xyz",
		FromLifecycle: AgentLifecycleActive, ToLifecycle: AgentLifecycleSuspended,
		Reason: "org_admin_suspended", RevisionBefore: 3, RevisionAfter: 4, RoomsChanged: 2,
	}
	raw, err := buildOrgAgentAuditPayload("org_agent_suspended", facts, time.Date(2026, 1, 2, 3, 4, 5, 0, time.UTC))
	if err != nil {
		t.Fatalf("buildOrgAgentAuditPayload: %v", err)
	}
	var payload map[string]any
	if err := json.Unmarshal(raw, &payload); err != nil {
		t.Fatalf("decode payload: %v", err)
	}

	wantTop := []string{
		"event_id", "occurred_at", "org_id", "user_id", "actor_role", "plane",
		"producer", "event", "subject", "resource_id", "outcome", "details",
	}
	assertExactKeys(t, "top-level payload", payload, wantTop)

	details, ok := payload["details"].(map[string]any)
	if !ok {
		t.Fatalf("details is not an object: %T", payload["details"])
	}
	wantDetails := []string{"from_lifecycle", "to_lifecycle", "reason", "revision_before", "revision_after", "rooms_changed"}
	assertExactKeys(t, "details", details, wantDetails)

	// Spot-check a few values to confirm it is the right facts, not just the right shape.
	if payload["org_id"] != "org-1" || payload["user_id"] != "user-1" || payload["resource_id"] != "agent-xyz" {
		t.Errorf("payload ids drifted: %+v", payload)
	}
	if details["from_lifecycle"] != AgentLifecycleActive || details["to_lifecycle"] != AgentLifecycleSuspended {
		t.Errorf("payload lifecycle fields drifted: %+v", details)
	}
}

func assertExactKeys(t *testing.T, name string, m map[string]any, want []string) {
	t.Helper()
	got := make([]string, 0, len(m))
	for k := range m {
		got = append(got, k)
	}
	sort.Strings(got)
	wantSorted := append([]string(nil), want...)
	sort.Strings(wantSorted)
	if len(got) != len(wantSorted) {
		t.Fatalf("%s: keys = %v, want exactly %v", name, got, wantSorted)
	}
	for i := range got {
		if got[i] != wantSorted[i] {
			t.Fatalf("%s: keys = %v, want exactly %v", name, got, wantSorted)
		}
	}
}

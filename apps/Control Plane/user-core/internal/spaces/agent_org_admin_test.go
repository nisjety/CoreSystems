package spaces

import (
	"encoding/json"
	"sort"
	"strings"
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

// D19/D22as: who may edit an agent's access. The owner and a listed manager
// always may; a canonical org owner or admin only for an ownerless org agent.
func TestAgentAccessEditAuthority(t *testing.T) {
	tests := []struct {
		name      string
		isOwner   bool
		access    string
		ownerless bool
		orgRole   string
		want      string
	}{
		{"owner", true, "", false, "", agentAccessAuthorityOwner},
		{"owner who is also an org admin", true, "", false, "admin", agentAccessAuthorityOwner},
		{"listed manager", false, AgentAccessManager, false, "member", agentAccessAuthorityManager},
		{"listed manager of an ownerless agent", false, AgentAccessManager, true, "admin", agentAccessAuthorityManager},
		{"org admin, ownerless agent", false, "", true, "admin", agentAccessAuthorityOrgAdmin},
		{"org owner, ownerless agent", false, "", true, "owner", agentAccessAuthorityOrgAdmin},
		{"org admin, owned agent", false, "", false, "admin", ""},
		{"org member, ownerless agent", false, "", true, "member", ""},
		{"org viewer, ownerless agent", false, "", true, "viewer", ""},
		{"compliance, ownerless agent", false, "", true, "compliance", ""},
		{"collaborator, ownerless agent", false, AgentAccessCollaborator, true, "member", ""},
		{"unresolved role, ownerless agent", false, "", true, "", ""},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := AgentAccessEditAuthority(test.isOwner, test.access, test.ownerless, test.orgRole); got != test.want {
				t.Fatalf("AgentAccessEditAuthority = %q, want %q", got, test.want)
			}
		})
	}
}

// D22as: only an owner-to-org flip of an owned agent gives up the owner.
func TestAgentAccessClearsOwner(t *testing.T) {
	tests := []struct {
		name     string
		hasOwner bool
		from, to string
		want     bool
	}{
		{"owner to org", true, AgentVisibilityOwner, AgentVisibilityOrg, true},
		{"owner stays owner", true, AgentVisibilityOwner, AgentVisibilityOwner, false},
		{"legacy owned org agent stays org", true, AgentVisibilityOrg, AgentVisibilityOrg, false},
		{"ownerless org agent", false, AgentVisibilityOrg, AgentVisibilityOrg, false},
		{"ownerless agent asked to become owner-visible", false, AgentVisibilityOrg, AgentVisibilityOwner, false},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := AgentAccessClearsOwner(test.hasOwner, test.from, test.to); got != test.want {
				t.Fatalf("AgentAccessClearsOwner = %v, want %v", got, test.want)
			}
		})
	}
}

// D19/D22as: the access-edit audit payload names policy words, counts and
// revisions, never a principal on the access list.
func TestBuildOrgAgentAccessAuditPayloadIsMinimal(t *testing.T) {
	raw, err := buildOrgAgentAccessAuditPayload(orgAgentAccessAuditFacts{
		OrgID: "org-1", ActorID: "user-1", ActorOrgRole: "admin", Authority: agentAccessAuthorityOrgAdmin,
		AgentID: "agent-xyz123", FromVisibility: AgentVisibilityOrg, ToVisibility: AgentVisibilityOrg,
		FromAddPolicy: AgentAddPolicyAnyone, ToAddPolicy: AgentAddPolicyNobody,
		EntriesChanged: true, EntryCount: 2, RevisionBefore: 4, RevisionAfter: 5,
	}, time.Date(2026, 11, 20, 3, 4, 5, 0, time.UTC))
	if err != nil {
		t.Fatalf("buildOrgAgentAccessAuditPayload: %v", err)
	}
	var payload map[string]any
	if err := json.Unmarshal(raw, &payload); err != nil {
		t.Fatalf("decode payload: %v", err)
	}
	assertExactKeys(t, "top-level payload", payload, []string{
		"event_id", "occurred_at", "org_id", "user_id", "actor_role", "plane",
		"producer", "event", "subject", "resource_id", "outcome", "details",
	})
	details, ok := payload["details"].(map[string]any)
	if !ok {
		t.Fatalf("details is not an object: %T", payload["details"])
	}
	assertExactKeys(t, "details", details, []string{
		"authority", "from_visibility", "to_visibility", "from_add_policy", "to_add_policy",
		"entries_changed", "entry_count", "owner_cleared", "revision_before", "revision_after",
	})
	if payload["event"] != "org_agent_access_changed" || payload["event_id"] != "org-agent-access:agent-xyz123:5" ||
		details["authority"] != "org_admin" || details["to_add_policy"] != AgentAddPolicyNobody {
		t.Fatalf("payload drifted: %+v", payload)
	}
	if !strings.HasSuffix(AuditSubjectOrgAgentAccessChanged, ".org_agent_access_changed") ||
		!strings.HasPrefix(AuditSubjectOrgAgentAccessChanged, "verevon.audit.v2.control.user-core.") {
		t.Fatalf("audit subject %q is outside user-core's authority prefix", AuditSubjectOrgAgentAccessChanged)
	}
}

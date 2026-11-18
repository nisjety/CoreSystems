package spaces

import "testing"

// WS17-S4: pure-function tests of the agent registry (agent_registry.go). No
// database: every fact AdmitDenial reads is constructed directly here.

// baseAllowFacts is a fact set AdmitDenial allows: an active managed room,
// an active org-visible agent anyone may add. Each subtest starts from a copy
// of it and flips exactly the facts needed to walk the WS17 D3 check order.
func baseAllowFacts() AgentAdmitFacts {
	return AgentAdmitFacts{
		SpaceActive:   true,
		SpaceKind:     KindRoom,
		ActorRoomRole: RoleManager,
		Agent: AgentSubject{
			Visibility: AgentVisibilityOrg,
			AddPolicy:  AgentAddPolicyAnyone,
			Lifecycle:  AgentLifecycleActive,
		},
	}
}

// TestAdmitDenialTable walks AdmitDenial's checks in the exact WS17 D3 order.
// The checks branch on visibility (an owner-visible agent can trigger
// agent_not_visible, personal_agents_not_entitled and owner_not_in_room; an
// add_policy_owner_only denial can only ever reach an org-visible agent,
// since an owner-visible agent that is not visible to the actor never
// reaches the add-policy switch, and an actor who IS visible to an
// owner-visible agent is always allowed by owner_only too). So the order is
// proven in two walks that together cover every one of the nine reasons,
// each starting from a fact set where every later check already passes.
func TestAdmitDenialTable(t *testing.T) {
	// Walk 1 (owner-visible agent): space_not_active, personal_space_not_supported,
	// room_role_insufficient, agent_suspended, agent_not_visible, add_policy_nobody,
	// personal_agents_not_entitled, owner_not_in_room, then allowed.
	f := baseAllowFacts()
	f.Agent.Visibility = AgentVisibilityOwner
	f.Agent.AddPolicy = AgentAddPolicyAnyone // skip the add-policy switch until its own step
	f.SpaceActive = false
	f.SpaceKind = KindPersonal
	f.ActorRoomRole = ""
	f.Agent.Lifecycle = AgentLifecycleSuspended
	f.ActorIsOwner = false
	f.ActorAccess = ""
	f.OrgPersonalAdmit = false
	f.OwnerActiveInRoom = false

	step := func(name, want string) {
		t.Helper()
		if got := AdmitDenial(f); got != want {
			t.Fatalf("%s: AdmitDenial = %q, want %q (facts=%+v)", name, got, want, f)
		}
	}
	step("1 space not active", AdmitReasonSpaceNotActive)
	f.SpaceActive = true
	step("2 personal space", AdmitReasonPersonalSpace)
	f.SpaceKind = KindRoom
	step("3 room role insufficient", AdmitReasonRoomRoleInsufficient)
	f.ActorRoomRole = RoleManager
	step("4 agent suspended", AdmitReasonAgentSuspended)
	f.Agent.Lifecycle = AgentLifecycleActive
	step("5 agent not visible", AdmitReasonAgentNotVisible)
	// The actor becomes the owner: visible now, and nobody still denies
	// everyone including the owner, so the next check is reachable.
	f.ActorIsOwner = true
	f.Agent.AddPolicy = AgentAddPolicyNobody
	step("6 add policy nobody", AdmitReasonAddPolicyNobody)
	f.Agent.AddPolicy = AgentAddPolicyAnyone
	step("7 personal agents not entitled", AdmitReasonNotEntitled)
	f.OrgPersonalAdmit = true
	step("8 owner not in room", AdmitReasonOwnerNotInRoom)
	f.OwnerActiveInRoom = true
	step("9 allowed", "")

	// Walk 2 (org-visible agent): space_not_active .. agent_suspended are the
	// same checks; agent_not_visible never triggers (the gate only applies to
	// an owner-visible agent); then add_policy_nobody, add_policy_owner_only,
	// then allowed -- an org-visible agent never reaches the owner-only tail.
	g := baseAllowFacts()
	g.SpaceActive = false
	g.SpaceKind = KindPersonal
	g.ActorRoomRole = ""
	g.Agent.Lifecycle = AgentLifecycleSuspended
	g.Agent.AddPolicy = AgentAddPolicyNobody
	g.ActorIsOwner = false
	g.ActorAccess = ""

	step2 := func(name, want string) {
		t.Helper()
		if got := AdmitDenial(g); got != want {
			t.Fatalf("%s: AdmitDenial = %q, want %q (facts=%+v)", name, got, want, g)
		}
	}
	step2("1 space not active", AdmitReasonSpaceNotActive)
	g.SpaceActive = true
	step2("2 personal space", AdmitReasonPersonalSpace)
	g.SpaceKind = KindRoom
	step2("3 room role insufficient", AdmitReasonRoomRoleInsufficient)
	g.ActorRoomRole = RoleManager
	step2("4 agent suspended", AdmitReasonAgentSuspended)
	g.Agent.Lifecycle = AgentLifecycleActive
	step2("5 add policy nobody (not agent_not_visible: org-visible is never gated)", AdmitReasonAddPolicyNobody)
	g.Agent.AddPolicy = AgentAddPolicyOwnerOnly
	step2("6 add policy owner only", AdmitReasonAddPolicyOwnerOnly)
	g.ActorAccess = AgentAccessCollaborator
	step2("7 allowed (listed collaborator satisfies owner_only; org-visible has no owner tail)", "")
}

// TestAddPolicyConstraint: "anyone" is valid only for an org-visible agent.
func TestAddPolicyConstraint(t *testing.T) {
	cases := []struct {
		name       string
		visibility string
		addPolicy  string
		wantErr    bool
		wantField  string
	}{
		{"org + anyone is valid", AgentVisibilityOrg, AgentAddPolicyAnyone, false, ""},
		{"owner + anyone is invalid", AgentVisibilityOwner, AgentAddPolicyAnyone, true, "add_policy"},
		{"owner + owner_only is valid", AgentVisibilityOwner, AgentAddPolicyOwnerOnly, false, ""},
		{"owner + nobody is valid", AgentVisibilityOwner, AgentAddPolicyNobody, false, ""},
		{"org + owner_only is valid", AgentVisibilityOrg, AgentAddPolicyOwnerOnly, false, ""},
		{"org + nobody is valid", AgentVisibilityOrg, AgentAddPolicyNobody, false, ""},
		{"unknown visibility", "bogus", AgentAddPolicyAnyone, true, "visibility"},
		{"unknown add_policy", AgentVisibilityOrg, "bogus", true, "add_policy"},
	}
	for _, tc := range cases {
		err := ValidateAgentPolicy(tc.visibility, tc.addPolicy)
		if tc.wantErr {
			roomErr, ok := err.(*RoomError)
			if !ok || roomErr == nil {
				t.Fatalf("%s: want a *RoomError, got %v", tc.name, err)
			}
			if roomErr.Status != 422 || roomErr.Code != CodeAgentPolicyInvalid {
				t.Fatalf("%s: want 422 %s, got %d %s", tc.name, CodeAgentPolicyInvalid, roomErr.Status, roomErr.Code)
			}
			if roomErr.Details["field"] != tc.wantField {
				t.Fatalf("%s: details.field = %v, want %q", tc.name, roomErr.Details["field"], tc.wantField)
			}
			continue
		}
		if err != nil {
			t.Fatalf("%s: want no error, got %v", tc.name, err)
		}
	}
}

// TestCanEditAgentAccess: its owner, or a manager on its access list, may
// edit an agent's visibility, add policy and access list.
func TestCanEditAgentAccess(t *testing.T) {
	cases := []struct {
		name         string
		actorIsOwner bool
		actorAccess  string
		want         bool
	}{
		{"owner", true, "", true},
		{"owner also listed as collaborator", true, AgentAccessCollaborator, true},
		{"listed manager", false, AgentAccessManager, true},
		{"listed collaborator", false, AgentAccessCollaborator, false},
		{"neither", false, "", false},
	}
	for _, tc := range cases {
		if got := CanEditAgentAccess(tc.actorIsOwner, tc.actorAccess); got != tc.want {
			t.Errorf("%s: CanEditAgentAccess = %v, want %v", tc.name, got, tc.want)
		}
	}
}

// TestCanWithdrawAgent: a room owner or manager, the agent's owner, or a
// manager on its access list may withdraw it. A room member below manager
// who is neither the agent's owner nor a listed manager may not.
func TestCanWithdrawAgent(t *testing.T) {
	cases := []struct {
		name          string
		actorRoomRole string
		actorIsOwner  bool
		actorAccess   string
		want          bool
	}{
		{"room owner", RoleOwner, false, "", true},
		{"room manager", RoleManager, false, "", true},
		{"room viewer but agent owner", RoleViewer, true, "", true},
		{"room editor but listed manager", RoleEditor, false, AgentAccessManager, true},
		{"room viewer, listed collaborator only", RoleViewer, false, AgentAccessCollaborator, false},
		{"room viewer, no rights at all", RoleViewer, false, "", false},
		{"room editor, no rights at all", RoleEditor, false, "", false},
		{"not a room member at all", "", false, "", false},
	}
	for _, tc := range cases {
		if got := CanWithdrawAgent(tc.actorRoomRole, tc.actorIsOwner, tc.actorAccess); got != tc.want {
			t.Errorf("%s: CanWithdrawAgent = %v, want %v", tc.name, got, tc.want)
		}
	}
}

// TestParseAgentAdmissionMode: unset and "off" both parse as off; "observe"
// and "enforce" parse as themselves; anything else, including a near-miss
// typo, is refused rather than silently defaulting.
func TestParseAgentAdmissionMode(t *testing.T) {
	okCases := []struct {
		raw  string
		want AgentAdmissionMode
	}{
		{"", AgentAdmissionOff},
		{"   ", AgentAdmissionOff},
		{"off", AgentAdmissionOff},
		{"observe", AgentAdmissionObserve},
		{"enforce", AgentAdmissionEnforce},
		{"  enforce  ", AgentAdmissionEnforce},
	}
	for _, tc := range okCases {
		got, err := ParseAgentAdmissionMode(tc.raw)
		if err != nil {
			t.Errorf("ParseAgentAdmissionMode(%q): unexpected error %v", tc.raw, err)
		}
		if got != tc.want {
			t.Errorf("ParseAgentAdmissionMode(%q) = %q, want %q", tc.raw, got, tc.want)
		}
	}
	for _, raw := range []string{"ENFORCE", "Observe", "obsErve", "enforced", "ofF", "bogus"} {
		if _, err := ParseAgentAdmissionMode(raw); err == nil {
			t.Errorf("ParseAgentAdmissionMode(%q): want a refusal, a typo must not silently run a different mode", raw)
		}
	}
}

// TestValidAgentSubjectID matches the migration 045 CHECK constraint.
func TestValidAgentSubjectID(t *testing.T) {
	valid := []string{"agent-abcdef", "agent-abc123", "agent-" + stringsRepeat("a", 64)}
	for _, id := range valid {
		if !ValidAgentSubjectID(id) {
			t.Errorf("ValidAgentSubjectID(%q) = false, want true", id)
		}
	}
	invalid := []string{"", "agent-", "agent-abcde", "notagent-abcdef", "Agent-abcdef", "agent-abc def", "agent-" + stringsRepeat("a", 65)}
	for _, id := range invalid {
		if ValidAgentSubjectID(id) {
			t.Errorf("ValidAgentSubjectID(%q) = true, want false", id)
		}
	}
}

// TestIsAgentSubjectID: only agent-* service subjects are gated by admission.
func TestIsAgentSubjectID(t *testing.T) {
	for _, id := range []string{"agent-abcdef", "agent-x"} {
		if !IsAgentSubjectID(id) {
			t.Errorf("IsAgentSubjectID(%q) = false, want true", id)
		}
	}
	for _, id := range []string{"worker-1", "integration-core", ""} {
		if IsAgentSubjectID(id) {
			t.Errorf("IsAgentSubjectID(%q) = true, want false", id)
		}
	}
}

func stringsRepeat(s string, n int) string {
	out := make([]byte, 0, n*len(s))
	for i := 0; i < n; i++ {
		out = append(out, s...)
	}
	return string(out)
}

package spaces

import (
	"context"
	"fmt"
	"testing"
)

// W3-DEC-ORGAGENT (D19) live proof of the org-admin agent lifecycle
// (agent_org_admin.go). Gated on TEST_DATABASE_URL through roomLiveFixture;
// skipped without it. txAuditOutbox is shared from effect_policy_audit_live_test.go.

func (f *roomLiveFixture) auditOutboxCount(t *testing.T, eventID string) int {
	t.Helper()
	var n int
	if err := f.pool.QueryRow(context.Background(),
		`SELECT count(*) FROM user_audit_outbox WHERE event_id=$1`, eventID).Scan(&n); err != nil {
		t.Fatalf("count audit outbox: %v", err)
	}
	return n
}

func (f *roomLiveFixture) agentDefinitionRevision(t *testing.T, agentID string) int64 {
	t.Helper()
	var rev int64
	if err := f.pool.QueryRow(context.Background(),
		`SELECT revision FROM agent_subjects WHERE agent_subject_id=$1`, agentID).Scan(&rev); err != nil {
		t.Fatalf("read agent revision: %v", err)
	}
	return rev
}

func TestOrgAgentLifecycleSuspendReactivateRetireLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	f.agentCleanup(t)
	ctx := context.Background()
	outbox := &txAuditOutbox{}
	f.repo.SetPolicyAuditOutbox(outbox)
	admin := f.user(t, "orgadmin-admin", f.org)

	room1 := f.staffedRoom(t, "orgadmin1")
	room2 := f.staffedRoom(t, "orgadmin2")
	id := f.agentID("orgwide")
	f.registerAgent(t, room1.owner, id, AgentVisibilityOrg, AgentAddPolicyAnyone)
	if _, err := f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: f.org, ActorID: room1.manager, SpaceRef: room1.ref, AgentSubjectID: id}); err != nil {
		t.Fatalf("admit room1: %v", err)
	}
	if _, err := f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: f.org, ActorID: room2.manager, SpaceRef: room2.ref, AgentSubjectID: id}); err != nil {
		t.Fatalf("admit room2: %v", err)
	}
	f.serviceAgent(t, room1.ref, id)
	f.serviceAgent(t, room2.ref, id)

	lifecycleReq := func(action string, expected int64) OrgAgentLifecycleRequest {
		return OrgAgentLifecycleRequest{
			OrgID: f.org, ActorID: admin, ActorOrgRole: "admin",
			AgentSubjectID: id, Action: action, ExpectedRevision: expected,
		}
	}

	// Suspend: both rooms move to suspended, both seats deactivate, one ledger
	// row per room, and exactly one audit-outbox row for the whole action.
	res, err := f.repo.ChangeOrgAgentLifecycle(ctx, lifecycleReq(OrgAgentActionSuspend, 1))
	if err != nil || !res.Changed || res.Lifecycle != AgentLifecycleSuspended || res.Revision != 2 || res.RoomsChanged != 2 {
		t.Fatalf("suspend: %+v %v", res, err)
	}
	for _, ref := range []string{room1.ref, room2.ref} {
		if got := f.admissionState(t, ref, id); got != AdmissionStateSuspended {
			t.Fatalf("room %s admission = %s, want suspended", ref, got)
		}
		if f.seatActive(t, ref, id) {
			t.Fatalf("room %s seat still active after suspend", ref)
		}
		if n := f.admissionEventCount(t, ref, id, AdmissionEventSuspended); n != 1 {
			t.Fatalf("room %s suspended ledger rows = %d, want 1", ref, n)
		}
	}
	if outbox.calls != 1 {
		t.Fatalf("audit outbox calls = %d, want exactly 1 for the whole suspend action", outbox.calls)
	}
	if n := f.auditOutboxCount(t, fmt.Sprintf("org-agent:%s:%d", id, 2)); n != 1 {
		t.Fatalf("audit outbox rows for the suspend event = %d, want 1", n)
	}

	// Reactivate restores the definition only: rooms stay suspended (no
	// auto-readmit), and a manager can admit the agent again normally.
	res, err = f.repo.ChangeOrgAgentLifecycle(ctx, lifecycleReq(OrgAgentActionReactivate, 2))
	if err != nil || !res.Changed || res.Lifecycle != AgentLifecycleActive || res.Revision != 3 || res.RoomsChanged != 0 {
		t.Fatalf("reactivate: %+v %v", res, err)
	}
	if got := f.admissionState(t, room1.ref, id); got != AdmissionStateSuspended {
		t.Fatalf("reactivate must not auto-readmit: room1 admission = %s", got)
	}
	if _, err := f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: f.org, ActorID: room1.manager, SpaceRef: room1.ref, AgentSubjectID: id}); err != nil {
		t.Fatalf("re-admit after reactivate: %v", err)
	}
	if got := f.admissionState(t, room1.ref, id); got != AdmissionStateAdmitted {
		t.Fatalf("room1 admission after re-admit = %s, want admitted", got)
	}

	// Retire withdraws every room row (room1 now admitted, room2 still
	// suspended -- both move) and is terminal.
	res, err = f.repo.ChangeOrgAgentLifecycle(ctx, lifecycleReq(OrgAgentActionRetire, 3))
	if err != nil || !res.Changed || res.Lifecycle != AgentLifecycleRetired || res.Revision != 4 || res.RoomsChanged != 2 {
		t.Fatalf("retire: %+v %v", res, err)
	}
	for _, ref := range []string{room1.ref, room2.ref} {
		if got := f.admissionState(t, ref, id); got != AdmissionStateWithdrawn {
			t.Fatalf("room %s admission after retire = %s, want withdrawn", ref, got)
		}
	}
	// Terminal: nothing can undo it.
	for _, action := range []string{OrgAgentActionSuspend, OrgAgentActionReactivate} {
		if _, err := f.repo.ChangeOrgAgentLifecycle(ctx, lifecycleReq(action, 4)); err == nil {
			t.Fatalf("%s on a retired agent must be refused", action)
		} else {
			wantRoomErr(t, err, 409, CodeAgentLifecycleInvalid)
		}
	}
	if f.agentDefinitionRevision(t, id) != 4 {
		t.Fatal("a refused lifecycle change on a retired agent advanced the revision")
	}
	// Retiring an already-retired agent changes nothing.
	again, err := f.repo.ChangeOrgAgentLifecycle(ctx, lifecycleReq(OrgAgentActionRetire, 4))
	if err != nil || again.Changed || again.Lifecycle != AgentLifecycleRetired {
		t.Fatalf("re-retiring: %+v %v", again, err)
	}
}

func TestOrgAgentLifecycleRevisionConflictAndVisibilityLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	f.agentCleanup(t)
	ctx := context.Background()
	f.repo.SetPolicyAuditOutbox(&txAuditOutbox{})
	admin := f.user(t, "orgadmin2-admin", f.org)
	owner := f.user(t, "orgadmin2-owner", f.org)

	orgID := f.agentID("stale")
	f.registerAgent(t, owner, orgID, AgentVisibilityOrg, AgentAddPolicyAnyone)
	_, err := f.repo.ChangeOrgAgentLifecycle(ctx, OrgAgentLifecycleRequest{
		OrgID: f.org, ActorID: admin, ActorOrgRole: "admin",
		AgentSubjectID: orgID, Action: OrgAgentActionSuspend, ExpectedRevision: 99,
	})
	stale := wantRoomErr(t, err, 409, CodeAgentStateChanged)
	if fmt.Sprint(stale.Details["current_revision"]) != "1" {
		t.Fatalf("current_revision = %v, want 1", stale.Details["current_revision"])
	}

	// An owner-visible agent is invisible to the org-admin route.
	personal := f.agentID("personal")
	f.registerAgent(t, owner, personal, AgentVisibilityOwner, AgentAddPolicyOwnerOnly)
	_, err = f.repo.ChangeOrgAgentLifecycle(ctx, OrgAgentLifecycleRequest{
		OrgID: f.org, ActorID: admin, ActorOrgRole: "admin",
		AgentSubjectID: personal, Action: OrgAgentActionSuspend, ExpectedRevision: 1,
	})
	wantRoomErr(t, err, 404, CodeAgentNotFound)
}

func TestListOrgAgentsOnlyOrgVisibilityInIDOrderLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	owner := f.user(t, "orgadmin3-owner", f.org)

	orgB := f.agentID("zzzlast")
	orgA := f.agentID("aaafirst")
	personal := f.agentID("personalhidden")
	f.registerAgent(t, owner, orgB, AgentVisibilityOrg, AgentAddPolicyAnyone)
	f.registerAgent(t, owner, orgA, AgentVisibilityOrg, AgentAddPolicyAnyone)
	f.registerAgent(t, owner, personal, AgentVisibilityOwner, AgentAddPolicyOwnerOnly)

	agents, err := f.repo.ListOrgAgents(ctx, f.org)
	if err != nil {
		t.Fatalf("list org agents: %v", err)
	}
	var ids []string
	for _, a := range agents {
		if a.AgentSubjectID == personal {
			t.Fatal("ListOrgAgents must never return an owner-visible agent")
		}
		if a.AgentSubjectID == orgA || a.AgentSubjectID == orgB {
			ids = append(ids, a.AgentSubjectID)
		}
	}
	if len(ids) != 2 || ids[0] != orgA || ids[1] != orgB {
		t.Fatalf("org-visible ids in order = %v, want [%s %s]", ids, orgA, orgB)
	}
}

// TestOrgAgentLifecycleFailsClosedWithoutPolicyAuditLive: without a wired
// policyAudit the repository refuses with 503 before opening a transaction,
// so no partial write ever happens.
func TestOrgAgentLifecycleFailsClosedWithoutPolicyAuditLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	f.agentCleanup(t)
	ctx := context.Background()
	// Deliberately NOT wiring SetPolicyAuditOutbox on this fixture's repo.
	s := f.staffedRoom(t, "noaudit")
	id := f.agentID("noaudit")
	f.registerAgent(t, s.owner, id, AgentVisibilityOrg, AgentAddPolicyAnyone)
	if _, err := f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: f.org, ActorID: s.manager, SpaceRef: s.ref, AgentSubjectID: id}); err != nil {
		t.Fatalf("admit: %v", err)
	}

	_, err := f.repo.ChangeOrgAgentLifecycle(ctx, OrgAgentLifecycleRequest{
		OrgID: f.org, ActorID: s.owner, ActorOrgRole: "owner",
		AgentSubjectID: id, Action: OrgAgentActionSuspend, ExpectedRevision: 1,
	})
	wantRoomErr(t, err, 503, CodeAgentRegistryUnavailable)
	if lifecycle, _ := f.agentLifecycle(t, id); lifecycle != AgentLifecycleActive {
		t.Fatalf("a fail-closed refusal wrote a partial change: lifecycle=%s", lifecycle)
	}
	if got := f.admissionState(t, s.ref, id); got != AdmissionStateAdmitted {
		t.Fatalf("a fail-closed refusal touched the room admission: %s", got)
	}
}

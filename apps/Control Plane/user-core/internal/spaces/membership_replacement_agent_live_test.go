package spaces

import (
	"context"
	"testing"
)

// WS17-S4 live proof of the roster's SPACE_AGENT_ADMISSION gate inside
// ReplaceMemberships/ReplaceMembershipsDetailed (repository.go), and of cascade
// B (ADR 4): a member's owner-visible agents leave a room with them, whether
// that departure comes from a roster sync going stale or a direct RemoveMember
// call. Gated on TEST_DATABASE_URL through roomLiveFixture; skipped without it.

// agentSeat inserts an active application_sync-sourced service membership, the
// shape a roster sync would have created before admission gating existed (or
// before the agent lost its admission). Unlike serviceAgent (member_ops), this
// row IS eligible for the generic stale-member sweep.
func (f *roomLiveFixture) agentSeat(t *testing.T, ref, subject, role string) {
	t.Helper()
	f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, active, granted_by, source)
		VALUES ($1,'service',$2,$3,TRUE,'application-space-membership-sync','application_sync')`, ref, subject, role)
}

func (f *roomLiveFixture) admissionState(t *testing.T, ref, agentID string) string {
	t.Helper()
	var state string
	if err := f.pool.QueryRow(context.Background(),
		`SELECT state FROM space_agent_admissions WHERE space_ref=$1 AND agent_subject_id=$2`, ref, agentID,
	).Scan(&state); err != nil {
		t.Fatalf("read admission state %s/%s: %v", ref, agentID, err)
	}
	return state
}

func (f *roomLiveFixture) seatActive(t *testing.T, ref, agentID string) bool {
	t.Helper()
	var active bool
	if err := f.pool.QueryRow(context.Background(),
		`SELECT active FROM space_memberships WHERE space_ref=$1 AND subject_type='service' AND subject_id=$2`, ref, agentID,
	).Scan(&active); err != nil {
		t.Fatalf("read seat %s/%s: %v", ref, agentID, err)
	}
	return active
}

func (f *roomLiveFixture) admissionEventCount(t *testing.T, ref, agentID, event string) int {
	t.Helper()
	var n int
	if err := f.pool.QueryRow(context.Background(),
		`SELECT count(*) FROM space_agent_admission_events WHERE space_ref=$1 AND agent_subject_id=$2 AND event=$3`,
		ref, agentID, event).Scan(&n); err != nil {
		t.Fatalf("count admission events: %v", err)
	}
	return n
}

// TestReplaceMembershipsSkipsUnadmittedAgentLive: enforce mode drops an
// unadmitted agent from the declared set (so the generic stale sweep removes
// its existing application_sync seat) and reports it in skipped_unadmitted.
func TestReplaceMembershipsSkipsUnadmittedAgentLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "enforce")
	id := f.agentID("unadm")
	f.registerAgent(t, s.owner, id, AgentVisibilityOrg, AgentAddPolicyAnyone)
	// Never admitted to this room. A seat exists anyway (sync drift, or an
	// admission that was later withdrawn).
	f.agentSeat(t, s.ref, id, "editor")

	f.repo.SetAgentAdmissionMode(AgentAdmissionEnforce)
	t.Cleanup(func() { f.repo.SetAgentAdmissionMode(AgentAdmissionOff) })

	detail, err := f.repo.ReplaceMembershipsDetailed(ctx, MembershipReplacement{
		SpaceRef: s.ref,
		Members:  []MemberGrant{{SubjectType: "service", SubjectID: id, Role: "editor"}},
	})
	if err != nil {
		t.Fatalf("replace: %v", err)
	}
	if len(detail.SkippedUnadmitted) != 1 || detail.SkippedUnadmitted[0] != id {
		t.Fatalf("skipped_unadmitted = %v, want [%s]", detail.SkippedUnadmitted, id)
	}
	if f.seatActive(t, s.ref, id) {
		t.Fatal("enforce mode must remove the existing seat of an unadmitted agent")
	}
}

// TestObserveModeLogsAndKeepsMemberLive: observe mode never removes a seat; it
// only appends a would_skip ledger row.
func TestObserveModeLogsAndKeepsMemberLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "observe")
	id := f.agentID("unadm")
	f.registerAgent(t, s.owner, id, AgentVisibilityOrg, AgentAddPolicyAnyone)
	f.agentSeat(t, s.ref, id, "editor")

	f.repo.SetAgentAdmissionMode(AgentAdmissionObserve)
	t.Cleanup(func() { f.repo.SetAgentAdmissionMode(AgentAdmissionOff) })

	detail, err := f.repo.ReplaceMembershipsDetailed(ctx, MembershipReplacement{
		SpaceRef: s.ref,
		Members:  []MemberGrant{{SubjectType: "service", SubjectID: id, Role: "editor"}},
	})
	if err != nil {
		t.Fatalf("replace: %v", err)
	}
	if len(detail.SkippedUnadmitted) != 0 {
		t.Fatalf("observe mode must never populate skipped_unadmitted, got %v", detail.SkippedUnadmitted)
	}
	if !f.seatActive(t, s.ref, id) {
		t.Fatal("observe mode must keep the seat of an unadmitted agent")
	}
	if n := f.admissionEventCount(t, s.ref, id, AdmissionEventWouldSkip); n != 1 {
		t.Fatalf("would_skip ledger rows = %d, want 1", n)
	}
}

// TestOwnerRemovalSuspendsPersonalAgentsInThatRoomLive: cascade B (ADR 4) runs
// identically whether a user leaves through the declarative roster sync going
// stale, or through a direct RemoveMember call: their owner-visible agent's
// admission in that room is suspended and its seat deactivated.
func TestOwnerRemovalSuspendsPersonalAgentsInThatRoomLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	f.agentCleanup(t)
	ctx := context.Background()
	f.agentPolicy(t, true)

	// Path 1: the declarative roster sync's stale-member pass.
	owner2 := f.user(t, "or-owner2", f.org)
	ref2 := f.room(t, "or2", owner2)
	agentOwner := f.user(t, "or-agentowner", f.org)
	// A sync-sourced manager row: eligible for the generic stale sweep, and
	// ranked high enough to self-admit their own personal agent.
	f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, active, granted_by, source)
		VALUES ($1,'user',$2,'manager',TRUE,'application-space-membership-sync','application_sync')`, ref2, agentOwner)
	idStale := f.agentID("stale")
	f.registerAgent(t, agentOwner, idStale, AgentVisibilityOwner, AgentAddPolicyOwnerOnly)
	if _, err := f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: f.org, ActorID: agentOwner, SpaceRef: ref2, AgentSubjectID: idStale}); err != nil {
		t.Fatalf("admit (stale path): %v", err)
	}
	f.serviceAgent(t, ref2, idStale)
	if got := f.admissionState(t, ref2, idStale); got != AdmissionStateAdmitted {
		t.Fatalf("precondition: admission state = %s, want admitted", got)
	}

	// A fresh declaration that omits agentOwner makes their sync-sourced row stale.
	if _, err := f.repo.ReplaceMembershipsDetailed(ctx, MembershipReplacement{SpaceRef: ref2, Members: nil}); err != nil {
		t.Fatalf("replace (drops agentOwner): %v", err)
	}
	if role, active, _ := f.role(t, ref2, agentOwner); active {
		t.Fatalf("agentOwner must have gone stale: role=%s active=%v", role, active)
	}
	if got := f.admissionState(t, ref2, idStale); got != AdmissionStateSuspended {
		t.Fatalf("stale path: admission state = %s, want suspended", got)
	}
	if f.seatActive(t, ref2, idStale) {
		t.Fatal("stale path: the agent's seat must be deactivated with its owner")
	}

	// Path 2: a direct RemoveMember call.
	s := f.staffedRoom(t, "or3")
	idDirect := f.agentID("direct")
	f.registerAgent(t, s.manager, idDirect, AgentVisibilityOwner, AgentAddPolicyOwnerOnly)
	if _, err := f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: f.org, ActorID: s.manager, SpaceRef: s.ref, AgentSubjectID: idDirect}); err != nil {
		t.Fatalf("admit (direct path): %v", err)
	}
	f.serviceAgent(t, s.ref, idDirect)

	if _, err := f.repo.RemoveMember(ctx, f.op(s.ref, s.owner, s.manager, "")); err != nil {
		t.Fatalf("remove manager: %v", err)
	}
	if got := f.admissionState(t, s.ref, idDirect); got != AdmissionStateSuspended {
		t.Fatalf("direct path: admission state = %s, want suspended", got)
	}
	if f.seatActive(t, s.ref, idDirect) {
		t.Fatal("direct path: the agent's seat must be deactivated with its owner")
	}
}

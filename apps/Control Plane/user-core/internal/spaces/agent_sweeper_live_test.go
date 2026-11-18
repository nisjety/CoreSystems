package spaces

import (
	"context"
	"testing"
)

// WS17-S4 cascade A (ADR 4) live proof of the owner-inactivity sweeper
// (agent_sweeper.go). Gated on TEST_DATABASE_URL through roomLiveFixture;
// skipped without it.

func (f *roomLiveFixture) agentLifecycle(t *testing.T, agentID string) (lifecycle, reason string) {
	t.Helper()
	if err := f.pool.QueryRow(context.Background(),
		`SELECT lifecycle, suspended_reason FROM agent_subjects WHERE agent_subject_id=$1`, agentID,
	).Scan(&lifecycle, &reason); err != nil {
		t.Fatalf("read agent lifecycle %s: %v", agentID, err)
	}
	return
}

func (f *roomLiveFixture) setOrgMemberStatus(t *testing.T, userID, status string) {
	t.Helper()
	f.exec(t, `UPDATE user_org_memberships SET status=$3 WHERE user_id=$1 AND org_id=$2`, userID, f.org, status)
}

// TestSweeperSuspendsAgentsOfInactiveOwnersLive: an agent whose owner is no
// longer an active organization member is suspended -- the definition, every
// room admission and every room seat -- in one pass, with no auto-resume once
// the owner comes back. A separate session already holding the sweeper's
// advisory lock proves a concurrent tick is skipped outright, not duplicated.
func TestSweeperSuspendsAgentsOfInactiveOwnersLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	f.agentCleanup(t)
	ctx := context.Background()
	f.agentPolicy(t, true)
	s := f.staffedRoom(t, "sweep")
	id := f.agentID("sweep1")
	// The manager owns a personal agent and, while still active, admits it
	// into their own room.
	f.registerAgent(t, s.manager, id, AgentVisibilityOwner, AgentAddPolicyOwnerOnly)
	if _, err := f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: f.org, ActorID: s.manager, SpaceRef: s.ref, AgentSubjectID: id}); err != nil {
		t.Fatalf("admit: %v", err)
	}
	f.serviceAgent(t, s.ref, id)

	// The owner leaves the organization (their membership is no longer active).
	f.setOrgMemberStatus(t, s.manager, "suspended")

	// A second session already holds the sweeper's leader lock: this tick must
	// be skipped outright -- zero suspended, no error, nothing touched.
	conn, err := f.pool.Acquire(ctx)
	if err != nil {
		t.Fatalf("acquire holder connection: %v", err)
	}
	holderTx, err := conn.Begin(ctx)
	if err != nil {
		t.Fatalf("begin holder tx: %v", err)
	}
	if _, err := holderTx.Exec(ctx, `SELECT pg_advisory_xact_lock($1)`, agentAdmissionSweepLock); err != nil {
		t.Fatalf("acquire sweeper lock in holder session: %v", err)
	}
	skipped, err := f.repo.SweepInactiveOwnerAgents(ctx)
	if err != nil || skipped != 0 {
		t.Fatalf("a tick that loses the leader election must do nothing: n=%d err=%v", skipped, err)
	}
	if lifecycle, _ := f.agentLifecycle(t, id); lifecycle != AgentLifecycleActive {
		t.Fatalf("a skipped tick touched the agent: lifecycle=%s", lifecycle)
	}
	_ = holderTx.Rollback(ctx)
	conn.Release()

	// Now the lock is free: the sweeper suspends the definition, the room
	// admission and the room seat together.
	suspended, err := f.repo.SweepInactiveOwnerAgents(ctx)
	if err != nil || suspended != 1 {
		t.Fatalf("sweep: n=%d err=%v, want 1", suspended, err)
	}
	if lifecycle, reason := f.agentLifecycle(t, id); lifecycle != AgentLifecycleSuspended || reason != "owner_inactive" {
		t.Fatalf("agent lifecycle = %s/%s, want suspended/owner_inactive", lifecycle, reason)
	}
	if got := f.admissionState(t, s.ref, id); got != AdmissionStateSuspended {
		t.Fatalf("admission state = %s, want suspended", got)
	}
	if f.seatActive(t, s.ref, id) {
		t.Fatal("the agent's seat must be deactivated by the sweep")
	}

	// No auto-resume: the owner returns, but nothing un-suspends automatically.
	f.setOrgMemberStatus(t, s.manager, "active")
	again, err := f.repo.SweepInactiveOwnerAgents(ctx)
	if err != nil || again != 0 {
		t.Fatalf("a re-run with the owner back must suspend nothing new: n=%d err=%v", again, err)
	}
	if lifecycle, _ := f.agentLifecycle(t, id); lifecycle != AgentLifecycleSuspended {
		t.Fatalf("the owner's return auto-resumed the agent: lifecycle=%s", lifecycle)
	}
}

// TestSweeperNeverSuspendsAnOwnerlessOrgVisibleAgentLive (W3-DEC-CTL1): the
// sweeper is gated on visibility='owner', not merely on owner_user_id being
// set, so an org-visible agent is never swept even in the legacy shape where
// its owner_user_id column still holds a value (e.g. after a visibility
// conversion -- UpdateAgentAccess never clears that column). An owner-visible
// agent of the very same departed owner IS suspended in the same pass. This
// is the key ownerless distinction CTL1 adds.
func TestSweeperNeverSuspendsAnOwnerlessOrgVisibleAgentLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	owner := f.user(t, "ctl1sweep-owner", f.org)
	orgAgentID := f.agentID("ctl1org")
	personalAgentID := f.agentID("ctl1personal")

	// An org-visible agent whose owner_user_id column still holds a value
	// (the legacy/converted shape): visibility alone must decide, not the
	// column's nullness.
	f.exec(t, `INSERT INTO agent_subjects (org_id, agent_subject_id, owner_user_id, visibility, add_policy, lifecycle, revision, registered_by)
		VALUES ($1,$2,$3,'org','anyone','active',1,$3)`, f.org, orgAgentID, owner)
	f.registerAgent(t, owner, personalAgentID, AgentVisibilityOwner, AgentAddPolicyOwnerOnly)

	f.setOrgMemberStatus(t, owner, "suspended")
	n, err := f.repo.SweepInactiveOwnerAgents(ctx)
	if err != nil || n != 1 {
		t.Fatalf("sweep: n=%d err=%v, want exactly 1 (the owner-visible agent only)", n, err)
	}
	if lifecycle, _ := f.agentLifecycle(t, orgAgentID); lifecycle != AgentLifecycleActive {
		t.Fatalf("an org-visible (ownerless) agent was suspended by the sweeper: lifecycle=%s", lifecycle)
	}
	if lifecycle, _ := f.agentLifecycle(t, personalAgentID); lifecycle != AgentLifecycleSuspended {
		t.Fatalf("the owner-visible agent of the same departed owner was not suspended: lifecycle=%s", lifecycle)
	}
}

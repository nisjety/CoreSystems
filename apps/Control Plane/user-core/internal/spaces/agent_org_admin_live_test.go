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

// D19/D22as live proof: a canonical org admin edits an ownerless org agent's
// access (one audit row), a plain member cannot, and an unverifiable org role
// fails closed with 503 and writes nothing.
func TestOrgAdminEditsAnOwnerlessOrgAgentAccessLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	outbox := &txAuditOutbox{}
	f.repo.SetPolicyAuditOutbox(outbox)
	registrar := f.user(t, "d22as-reg", f.org)
	admin := f.user(t, "d22as-admin", f.org)
	member := f.user(t, "d22as-member", f.org)
	id := f.agentID("d22asorg")
	f.registerAgent(t, registrar, id, AgentVisibilityOrg, AgentAddPolicyAnyone)
	role := func(r string, err error) func(context.Context) (string, error) {
		return func(context.Context) (string, error) { return r, err }
	}
	edit := func(actor string, resolve func(context.Context) (string, error)) (*AgentView, error) {
		return f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
			OrgID: f.org, ActorID: actor, AgentSubjectID: id, ExpectedRevision: 1,
			Visibility: AgentVisibilityOrg, AddPolicy: AgentAddPolicyNobody,
			Entries:        []AgentAccessEntry{{PrincipalID: member, Role: AgentAccessManager}},
			ResolveOrgRole: resolve,
		})
	}

	_, err := edit(member, role("member", nil))
	wantRoomErr(t, err, 403, CodeAgentAccessForbidden)
	_, err = edit(admin, role("", fmt.Errorf("authority down")))
	wantRoomErr(t, err, 503, CodeAgentRegistryUnavailable)
	if outbox.calls != 0 || f.agentDefinitionRevision(t, id) != 1 {
		t.Fatalf("a refused edit wrote: outbox calls=%d revision=%d", outbox.calls, f.agentDefinitionRevision(t, id))
	}

	view, err := edit(admin, role("admin", nil))
	if err != nil || view.Revision != 2 || view.AddPolicy != AgentAddPolicyNobody || view.OwnerUserID != "" {
		t.Fatalf("org admin edit: %+v %v", view, err)
	}
	if outbox.calls != 1 || f.auditOutboxCount(t, fmt.Sprintf("org-agent-access:%s:2", id)) != 1 {
		t.Fatalf("org admin edit must write exactly one audit row: calls=%d", outbox.calls)
	}
	// The member is now a listed manager and edits without any org-role lookup or audit.
	if _, err := f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: member, AgentSubjectID: id, ExpectedRevision: 2,
		Visibility: AgentVisibilityOrg, AddPolicy: AgentAddPolicyAnyone,
		Entries: []AgentAccessEntry{{PrincipalID: member, Role: AgentAccessManager}},
		ResolveOrgRole: func(context.Context) (string, error) {
			t.Fatal("a listed manager's edit must not read the org role")
			return "", nil
		},
	}); err != nil {
		t.Fatalf("listed manager edit: %v", err)
	}
	if outbox.calls != 1 {
		t.Fatalf("a manager's edit is not an org-admin edit: outbox calls=%d", outbox.calls)
	}
}

// D22as live proof: an owner flipping their agent from owner to org visibility
// gives up the owner (the column is NULL afterwards), with one audit row; after
// that an org admin may edit it like any ownerless org agent.
func TestOwnerToOrgFlipClearsTheOwnerLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	outbox := &txAuditOutbox{}
	f.repo.SetPolicyAuditOutbox(outbox)
	owner := f.user(t, "d22as-owner", f.org)
	admin := f.user(t, "d22as-admin2", f.org)
	id := f.agentID("d22asflip")
	f.registerAgent(t, owner, id, AgentVisibilityOwner, AgentAddPolicyOwnerOnly)

	view, err := f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: id, ExpectedRevision: 1,
		Visibility: AgentVisibilityOrg, AddPolicy: AgentAddPolicyAnyone,
	})
	if err != nil || view.OwnerUserID != "" || view.Visibility != AgentVisibilityOrg || view.Revision != 2 {
		t.Fatalf("owner-to-org flip: %+v %v", view, err)
	}
	var ownerCol *string
	if err := f.pool.QueryRow(ctx, `SELECT owner_user_id FROM agent_subjects WHERE agent_subject_id=$1`, id).Scan(&ownerCol); err != nil {
		t.Fatalf("read row: %v", err)
	}
	if ownerCol != nil {
		t.Fatalf("owner_user_id = %q after the flip, want NULL", *ownerCol)
	}
	if outbox.calls != 1 {
		t.Fatalf("the owner clear must write one audit row: calls=%d", outbox.calls)
	}
	if _, err := f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: admin, AgentSubjectID: id, ExpectedRevision: 2,
		Visibility: AgentVisibilityOrg, AddPolicy: AgentAddPolicyNobody,
		ResolveOrgRole: func(context.Context) (string, error) { return "owner", nil },
	}); err != nil {
		t.Fatalf("org owner edits the now ownerless agent: %v", err)
	}
}

// D24: a flip submitted with no entries (as above) must not lock the former owner out.
// They stay on the access list as a manager, so they can still edit the agent they just
// gave up ownership of, without needing an org owner/admin to restore their access.
func TestOwnerToOrgFlipKeepsTheFormerOwnerAsManagerLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	outbox := &txAuditOutbox{}
	f.repo.SetPolicyAuditOutbox(outbox)
	owner := f.user(t, "d24-owner", f.org)
	id := f.agentID("d24flip")
	f.registerAgent(t, owner, id, AgentVisibilityOwner, AgentAddPolicyOwnerOnly)

	if _, err := f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: id, ExpectedRevision: 1,
		Visibility: AgentVisibilityOrg, AddPolicy: AgentAddPolicyAnyone,
	}); err != nil {
		t.Fatalf("owner-to-org flip with no entries: %v", err)
	}
	var role string
	if err := f.pool.QueryRow(ctx,
		`SELECT role FROM agent_subject_access WHERE org_id=$1 AND agent_subject_id=$2 AND principal_id=$3`,
		f.org, id, owner).Scan(&role); err != nil {
		t.Fatalf("former owner must be on the access list after the flip: %v", err)
	}
	if role != AgentAccessManager {
		t.Fatalf("former owner's role = %q, want manager", role)
	}
	// Now ownerless with the former owner as manager: they edit it like any manager, no
	// ResolveOrgRole needed, and flipping again does not duplicate their entry.
	if _, err := f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: id, ExpectedRevision: 2,
		Visibility: AgentVisibilityOrg, AddPolicy: AgentAddPolicyNobody,
		Entries: []AgentAccessEntry{{PrincipalID: owner, Role: AgentAccessManager}},
	}); err != nil {
		t.Fatalf("former owner, now a manager, edits the agent: %v", err)
	}
	var count int
	if err := f.pool.QueryRow(ctx,
		`SELECT count(*) FROM agent_subject_access WHERE org_id=$1 AND agent_subject_id=$2 AND principal_id=$3`,
		f.org, id, owner).Scan(&count); err != nil {
		t.Fatalf("count access rows: %v", err)
	}
	if count != 1 {
		t.Fatalf("former owner's access row count = %d, want exactly 1 (no duplicate)", count)
	}
}

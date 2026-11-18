package spaces

import (
	"context"
	"fmt"
	"strings"
	"testing"
)

// WS17-S4 (plus its W3-DEC-CTL1 ownerless extension below) live proof of the
// agent registry repository (agent_repository.go). Gated on TEST_DATABASE_URL
// through roomLiveFixture (membership_ops_live_test.go); skipped without it.

// agentID returns a well-formed agent subject id (migration 045's CHECK
// constraint) scoped to this fixture's run, so cleanup can find it by prefix.
func (f *roomLiveFixture) agentID(name string) string {
	return "agent-" + f.suffix + "-" + name
}

// agentCleanup removes every row this file's tests may have written, scoped
// by the agent id prefix agentID produces.
func (f *roomLiveFixture) agentCleanup(t *testing.T) {
	t.Helper()
	t.Cleanup(func() {
		ctx := context.Background()
		like := "agent-" + f.suffix + "-%"
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_agent_admission_events WHERE agent_subject_id LIKE $1`, like)
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_agent_admissions WHERE agent_subject_id LIKE $1`, like)
		_, _ = f.pool.Exec(ctx, `DELETE FROM agent_subject_access WHERE agent_subject_id LIKE $1`, like)
		_, _ = f.pool.Exec(ctx, `DELETE FROM agent_subjects WHERE agent_subject_id LIKE $1`, like)
	})
}

// agentPolicy ensures an effect-policy row for f.org exists and sets its
// personal_agent_admit_entitled bit.
func (f *roomLiveFixture) agentPolicy(t *testing.T, personalAdmit bool) {
	t.Helper()
	f.exec(t, `INSERT INTO space_effect_policies (org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
		retention_class, residency, deletion_scope) VALUES ($1,'privacy:'||$1||':1','assistant_collaboration','contract','internal','standard','swedencentral','space')
		ON CONFLICT (org_id) DO NOTHING`, f.org)
	f.exec(t, `UPDATE space_effect_policies SET personal_agent_admit_entitled=$2 WHERE org_id=$1`, f.org, personalAdmit)
}

// registerAgent registers subject as a fully-formed request and fails the
// test on error, returning the created view.
func (f *roomLiveFixture) registerAgent(t *testing.T, actor, id, visibility, addPolicy string) *AgentView {
	t.Helper()
	view, _, err := f.repo.RegisterAgentSubject(context.Background(), AgentRegisterRequest{
		OrgID: f.org, ActorID: actor, AgentSubjectID: id, Visibility: visibility, AddPolicy: addPolicy,
	})
	if err != nil {
		t.Fatalf("register agent %s: %v", id, err)
	}
	return view
}

func TestRegisterIsOwnerDerivedAndFirstWriterWinsLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	owner := f.user(t, "reg-owner", f.org)
	other := f.user(t, "reg-other", f.org)
	foreign := f.user(t, "reg-foreign", f.other)
	id := f.agentID("reg1")

	// First registration creates an owner-derived definition.
	view, created, err := f.repo.RegisterAgentSubject(ctx, AgentRegisterRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: id, Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
	})
	if err != nil || !created || view.OwnerUserID != owner || view.Role != "owner" || view.Revision != 1 {
		t.Fatalf("first registration: %+v created=%v err=%v", view, created, err)
	}

	// The same owner replaying it is a 200 (created=false), the stored fact.
	replay, created, err := f.repo.RegisterAgentSubject(ctx, AgentRegisterRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: id, Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
	})
	if err != nil || created || replay.OwnerUserID != owner {
		t.Fatalf("owner replay: %+v created=%v err=%v", replay, created, err)
	}

	// A different owner, same org, same id: 409 without revealing who owns it.
	_, _, err = f.repo.RegisterAgentSubject(ctx, AgentRegisterRequest{
		OrgID: f.org, ActorID: other, AgentSubjectID: id, Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
	})
	conflict := wantRoomErr(t, err, 409, CodeAgentSubjectConflict)
	if len(conflict.Details) != 0 || conflict.Message == "" || strings.Contains(conflict.Message, owner) {
		t.Fatalf("a conflicting registration leaked the real owner: %+v", conflict)
	}

	// Another organization, same id (global uniqueness): 409 the same way.
	_, _, err = f.repo.RegisterAgentSubject(ctx, AgentRegisterRequest{
		OrgID: f.other, ActorID: foreign, AgentSubjectID: id, Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
	})
	wantRoomErr(t, err, 409, CodeAgentSubjectConflict)

	// Missing delegation.
	_, _, err = f.repo.RegisterAgentSubject(ctx, AgentRegisterRequest{ActorID: owner, AgentSubjectID: id})
	wantRoomErr(t, err, 403, CodeAgentDelegationRequired)

	// A malformed id is refused before anything is written.
	_, _, err = f.repo.RegisterAgentSubject(ctx, AgentRegisterRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: "not-an-agent-id", Visibility: AgentVisibilityOwner,
	})
	roomErr := wantRoomErr(t, err, 422, CodeAgentPolicyInvalid)
	if roomErr.Details["field"] != "agent_subject_id" {
		t.Fatalf("details.field = %v", roomErr.Details["field"])
	}
}

func TestAccessUpdateRequiresExpectedRevisionLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	owner := f.user(t, "acc-owner", f.org)
	nonEditor := f.user(t, "acc-nonedit", f.org)
	outsider := f.user(t, "acc-outsider", "")
	id := f.agentID("acc1")
	f.registerAgent(t, owner, id, AgentVisibilityOwner, AgentAddPolicyOwnerOnly)

	// A totally unlisted org member cannot even see an owner-visible agent,
	// so an edit attempt answers like an unknown id, not a bare refusal.
	_, err := f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: nonEditor, AgentSubjectID: id,
		Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly, ExpectedRevision: 1,
	})
	wantRoomErr(t, err, 404, CodeAgentNotFound)

	// The owner with a stale revision gets 409 and the current revision.
	_, err = f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: id,
		Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly, ExpectedRevision: 9,
	})
	stale := wantRoomErr(t, err, 409, CodeAgentStateChanged)
	if fmt.Sprint(stale.Details["current_revision"]) != "1" {
		t.Fatalf("current_revision = %v, want 1", stale.Details["current_revision"])
	}

	// A principal who is not an active organization member cannot be listed.
	_, err = f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: id, ExpectedRevision: 1,
		Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
		Entries: []AgentAccessEntry{{PrincipalID: outsider, Role: AgentAccessManager}},
	})
	wantRoomErr(t, err, 422, CodePrincipalNotInOrg)

	// The owner, at the right revision, lists nonEditor as a collaborator: now
	// visible, but a collaborator may not edit the agent -- a true 403, distinct
	// from the 404 an unlisted outsider gets.
	view, err := f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: id, ExpectedRevision: 1,
		Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
		Entries: []AgentAccessEntry{{PrincipalID: nonEditor, Role: AgentAccessCollaborator}},
	})
	if err != nil || view.Revision != 2 {
		t.Fatalf("owner edit at the right revision: %+v %v", view, err)
	}
	_, err = f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: nonEditor, AgentSubjectID: id, ExpectedRevision: 2,
		Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
		Entries: []AgentAccessEntry{{PrincipalID: nonEditor, Role: AgentAccessCollaborator}},
	})
	wantRoomErr(t, err, 403, CodeAgentAccessForbidden)

	// Promoted to manager, nonEditor may edit.
	if _, err := f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: id, ExpectedRevision: 2,
		Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
		Entries: []AgentAccessEntry{{PrincipalID: nonEditor, Role: AgentAccessManager}},
	}); err != nil {
		t.Fatalf("owner promotes nonEditor to manager: %v", err)
	}
	if _, err := f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: nonEditor, AgentSubjectID: id, ExpectedRevision: 3,
		Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
		Entries: []AgentAccessEntry{{PrincipalID: nonEditor, Role: AgentAccessManager}},
	}); err != nil {
		t.Fatalf("listed manager edit: %v", err)
	}

	// An unknown agent answers like any other unknown id.
	_, err = f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: f.agentID("nosuch"), ExpectedRevision: 1,
		Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
	})
	wantRoomErr(t, err, 404, CodeAgentNotFound)
}

func TestAgentAdmissionMatrixLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	f.agentCleanup(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "matrix")
	outsider := f.user(t, "matrix-outsider", f.org)
	foreign := f.user(t, "matrix-foreign", f.other)
	f.agentPolicy(t, true)
	id := f.agentID("orgagent")
	f.registerAgent(t, s.owner, id, AgentVisibilityOrg, AgentAddPolicyAnyone)

	admit := func(actor, orgID, spaceRef, agentID string) (*AgentAdmitResult, error) {
		return f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: orgID, ActorID: actor, SpaceRef: spaceRef, AgentSubjectID: agentID})
	}

	// editor and viewer: insufficient room role.
	for _, actor := range []string{s.editor, s.viewer} {
		_, err := admit(actor, f.org, s.ref, id)
		roomErr := wantRoomErr(t, err, 403, CodeAgentNotAddable)
		if roomErr.Details["reason"] != AdmitReasonRoomRoleInsufficient {
			t.Fatalf("actor %s: reason = %v, want %s", actor, roomErr.Details["reason"], AdmitReasonRoomRoleInsufficient)
		}
	}
	// an org member who is not in the room at all: room_not_found.
	_, err := admit(outsider, f.org, s.ref, id)
	wantRoomErr(t, err, 404, CodeRoomNotFound)
	// another organization's member, claiming this org: room_not_found, never agent-specific.
	_, err = admit(foreign, f.org, s.ref, id)
	wantRoomErr(t, err, 404, CodeRoomNotFound)
	// the room under the wrong org: room_not_found.
	_, err = admit(s.owner, f.other, s.ref, id)
	wantRoomErr(t, err, 404, CodeRoomNotFound)
	// an unregistered (invisible) agent: agent_not_found, never agent_not_addable.
	_, err = admit(s.owner, f.org, s.ref, f.agentID("nosuch"))
	wantRoomErr(t, err, 404, CodeAgentNotFound)

	// manager and owner: both may admit.
	for _, actor := range []string{s.manager, s.owner} {
		res, err := admit(actor, f.org, s.ref, id)
		if err != nil {
			t.Fatalf("actor %s admit: %v", actor, err)
		}
		if actor == s.manager && !res.Changed {
			t.Fatalf("first admission must report changed=true")
		}
		if res.State != AdmissionStateAdmitted {
			t.Fatalf("actor %s: state = %s", actor, res.State)
		}
	}
}

func TestAgentAdmissionIdempotencyLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	f.agentCleanup(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "idem")
	f.agentPolicy(t, true)
	idA := f.agentID("idema")
	idB := f.agentID("idemb")
	f.registerAgent(t, s.owner, idA, AgentVisibilityOrg, AgentAddPolicyAnyone)
	f.registerAgent(t, s.owner, idB, AgentVisibilityOrg, AgentAddPolicyAnyone)

	key := "idem-key-1"
	first, err := f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: f.org, ActorID: s.manager, SpaceRef: s.ref, AgentSubjectID: idA, IdempotencyKey: key})
	if err != nil || !first.Changed || first.Replayed {
		t.Fatalf("first admission: %+v %v", first, err)
	}
	// Replaying the same key for the same agent is idempotent: not re-changed.
	again, err := f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: f.org, ActorID: s.manager, SpaceRef: s.ref, AgentSubjectID: idA, IdempotencyKey: key})
	if err != nil || again.Changed || !again.Replayed || again.State != AdmissionStateAdmitted {
		t.Fatalf("replay: %+v %v", again, err)
	}
	// The same key for a different agent is a conflict.
	_, err = f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: f.org, ActorID: s.manager, SpaceRef: s.ref, AgentSubjectID: idB, IdempotencyKey: key})
	wantRoomErr(t, err, 409, CodeIdempotencyKeyReused)
}

// TestAgentWithdrawalLive: withdrawal is rights-gated (a plain viewer may not;
// the agent's owner may even with an insufficient room role), and the agent's
// service seat is deactivated in the same transaction as the withdrawal.
func TestAgentWithdrawalLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	f.agentCleanup(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "withdraw")
	f.agentPolicy(t, true)
	id := f.agentID("wd1")
	// The manager owns a personal agent and, while still a manager, admits it themselves.
	f.registerAgent(t, s.manager, id, AgentVisibilityOwner, AgentAddPolicyOwnerOnly)
	if _, err := f.repo.AdmitAgent(ctx, AgentAdmitRequest{OrgID: f.org, ActorID: s.manager, SpaceRef: s.ref, AgentSubjectID: id}); err != nil {
		t.Fatalf("admit: %v", err)
	}
	// Give the agent an actual seat in the room, as the roster would hold once admitted.
	f.serviceAgent(t, s.ref, id)

	// A plain viewer, neither the owner nor listed, may not withdraw it.
	_, err := f.repo.WithdrawAgent(ctx, AgentWithdrawRequest{OrgID: f.org, ActorID: s.viewer, SpaceRef: s.ref, AgentSubjectID: id})
	wantRoomErr(t, err, 403, CodeAgentWithdrawForbidden)
	var seatActive bool
	if err := f.pool.QueryRow(ctx, `SELECT active FROM space_memberships WHERE space_ref=$1 AND subject_type='service' AND subject_id=$2`, s.ref, id).Scan(&seatActive); err != nil {
		t.Fatalf("read seat: %v", err)
	}
	if !seatActive {
		t.Fatal("a refused withdrawal deactivated the seat")
	}

	// Now the manager's ROOM role is downgraded to viewer: CanManageRoom would
	// refuse them, but they are still the agent's owner, so withdrawal still works.
	f.exec(t, `UPDATE space_memberships SET role='viewer' WHERE space_ref=$1 AND subject_type='user' AND subject_id=$2`, s.ref, s.manager)
	res, err := f.repo.WithdrawAgent(ctx, AgentWithdrawRequest{OrgID: f.org, ActorID: s.manager, SpaceRef: s.ref, AgentSubjectID: id})
	if err != nil || !res.Changed || res.State != AdmissionStateWithdrawn {
		t.Fatalf("owner withdrawal despite a low room role: %+v %v", res, err)
	}
	if err := f.pool.QueryRow(ctx, `SELECT active FROM space_memberships WHERE space_ref=$1 AND subject_type='service' AND subject_id=$2`, s.ref, id).Scan(&seatActive); err != nil {
		t.Fatalf("read seat after withdrawal: %v", err)
	}
	if seatActive {
		t.Fatal("the agent's seat must be deactivated in the same transaction as the withdrawal")
	}
}

// TestAgentCandidatesNeverRevealAnInvisibleAgentLive: an unregistered id, a
// malformed id and an agent the caller cannot see must all answer exactly
// alike -- not visible, cannot add, reason agent_not_visible.
func TestAgentCandidatesNeverRevealAnInvisibleAgentLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	f.agentCleanup(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "cand")
	f.agentPolicy(t, true)
	hidden := f.agentID("hidden")
	f.registerAgent(t, s.owner, hidden, AgentVisibilityOwner, AgentAddPolicyOwnerOnly) // not viewer's, not listed to viewer
	visible := f.agentID("visible")
	f.registerAgent(t, s.owner, visible, AgentVisibilityOrg, AgentAddPolicyAnyone)
	unregistered := f.agentID("nosuch")

	candidates, err := f.repo.AgentCandidates(ctx, f.org, s.viewer, s.ref, []string{hidden, visible, unregistered, "not-an-agent-id"})
	if err != nil {
		t.Fatalf("candidates: %v", err)
	}
	byID := map[string]AgentCandidate{}
	for _, c := range candidates {
		byID[c.AgentSubjectID] = c
	}
	for _, id := range []string{hidden, unregistered, "not-an-agent-id"} {
		c, ok := byID[id]
		if !ok {
			t.Fatalf("candidate %s missing from the answer", id)
		}
		if c.Visible || c.CanAdd || c.Reason != AdmitReasonAgentNotVisible {
			t.Fatalf("candidate %s = %+v, want invisible/cannot-add/agent_not_visible for all three alike", id, c)
		}
	}
	if c := byID[visible]; !c.Visible {
		t.Fatalf("the org-visible agent must be visible to any room member: %+v", c)
	}
}

// === W3-DEC-CTL1 (ownerless org agent) extensions below ===

// TestRegisterOrgVisibleAgentIsOwnerlessLive: an org-visible registration
// stores owner_user_id NULL and registered_by the actor (D10), and -- unlike
// the owner-visible first-writer-wins rule -- a DIFFERENT active org member
// may replay the same registration, because there is no single owner to
// protect. A non-member may not register (or replay-register) it at all.
func TestRegisterOrgVisibleAgentIsOwnerlessLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	member1 := f.user(t, "ctl1-member1", f.org)
	member2 := f.user(t, "ctl1-member2", f.org)
	nonMember := f.user(t, "ctl1-nonmember", "")
	id := f.agentID("ownerless1")

	view, created, err := f.repo.RegisterAgentSubject(ctx, AgentRegisterRequest{
		OrgID: f.org, ActorID: member1, AgentSubjectID: id, Visibility: AgentVisibilityOrg, AddPolicy: AgentAddPolicyAnyone,
	})
	if err != nil || !created || view.OwnerUserID != "" || view.Role != "member" {
		t.Fatalf("org-visible registration: %+v created=%v err=%v", view, created, err)
	}
	var ownerCol *string
	var registeredBy string
	if err := f.pool.QueryRow(ctx, `SELECT owner_user_id, registered_by FROM agent_subjects WHERE agent_subject_id=$1`, id).Scan(&ownerCol, &registeredBy); err != nil {
		t.Fatalf("read row: %v", err)
	}
	if ownerCol != nil {
		t.Fatalf("owner_user_id = %v, want NULL for an org-visible (ownerless) agent", *ownerCol)
	}
	if registeredBy != member1 {
		t.Fatalf("registered_by = %q, want %q", registeredBy, member1)
	}

	// Ownerless semantics: a DIFFERENT active org member replaying the same
	// org-visible registration also succeeds (200, created=false) -- there is
	// no single owner to protect, unlike the owner-visible case.
	replay, created, err := f.repo.RegisterAgentSubject(ctx, AgentRegisterRequest{
		OrgID: f.org, ActorID: member2, AgentSubjectID: id, Visibility: AgentVisibilityOrg, AddPolicy: AgentAddPolicyAnyone,
	})
	if err != nil || created || replay.OwnerUserID != "" {
		t.Fatalf("a different member's replay of an ownerless registration: %+v created=%v err=%v", replay, created, err)
	}

	// A non-member may not register a brand new org-visible agent, nor
	// replay-register an existing one.
	idForbidden := f.agentID("ownerless2")
	_, _, err = f.repo.RegisterAgentSubject(ctx, AgentRegisterRequest{
		OrgID: f.org, ActorID: nonMember, AgentSubjectID: idForbidden, Visibility: AgentVisibilityOrg, AddPolicy: AgentAddPolicyAnyone,
	})
	wantRoomErr(t, err, 403, CodeAgentAccessForbidden)
	_, _, err = f.repo.RegisterAgentSubject(ctx, AgentRegisterRequest{
		OrgID: f.org, ActorID: nonMember, AgentSubjectID: id, Visibility: AgentVisibilityOrg, AddPolicy: AgentAddPolicyAnyone,
	})
	wantRoomErr(t, err, 403, CodeAgentAccessForbidden)
}

// TestRegisterOwnerVisibleAgentStaysOwnerDerivedLive: a CTL1 regression check
// -- an owner-visible registration is unchanged: it stores a real owner and
// stays first-writer-wins (see TestRegisterIsOwnerDerivedAndFirstWriterWinsLive
// for the full base-layer table; this just confirms the row shape directly).
func TestRegisterOwnerVisibleAgentStaysOwnerDerivedLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	owner := f.user(t, "ctl1-ownerderived", f.org)
	id := f.agentID("ownerderived")
	view, created, err := f.repo.RegisterAgentSubject(ctx, AgentRegisterRequest{
		OrgID: f.org, ActorID: owner, AgentSubjectID: id, Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
	})
	if err != nil || !created || view.OwnerUserID != owner || view.Role != "owner" {
		t.Fatalf("owner-visible registration: %+v created=%v err=%v", view, created, err)
	}
	var ownerCol *string
	if err := f.pool.QueryRow(ctx, `SELECT owner_user_id FROM agent_subjects WHERE agent_subject_id=$1`, id).Scan(&ownerCol); err != nil {
		t.Fatalf("read row: %v", err)
	}
	if ownerCol == nil || *ownerCol != owner {
		t.Fatalf("owner_user_id = %v, want %q", ownerCol, owner)
	}
}

// TestGrantingOwnerVisibilityOnAnOwnerlessAgentLive: UpdateAgentAccess refuses
// 422 agent_policy_invalid when the caller tries to turn an ownerless
// (owner_user_id NULL) agent into an owner-visible one -- D10's "an
// organization agent has no owner, so it cannot become owner-visible" rule.
//
// NOTE: in production, no path grants the first manager entry on a truly
// ownerless agent (CanEditAgentAccess needs isOwner or a listed manager, and
// nothing ever lists one on an owner_user_id=NULL row), so this case cannot
// currently arise through the HTTP surface. The access-list row is seeded
// directly here to exercise the guard itself in isolation; see the test
// report for this observation.
func TestGrantingOwnerVisibilityOnAnOwnerlessAgentLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.agentCleanup(t)
	ctx := context.Background()
	member := f.user(t, "ctl1-grant", f.org)
	id := f.agentID("ownerless3")
	f.registerAgent(t, member, id, AgentVisibilityOrg, AgentAddPolicyAnyone)
	f.exec(t, `INSERT INTO agent_subject_access (org_id, agent_subject_id, principal_type, principal_id, role, granted_by)
		VALUES ($1,$2,'user',$3,'manager','test-setup')`, f.org, id, member)

	_, err := f.repo.UpdateAgentAccess(ctx, AgentAccessRequest{
		OrgID: f.org, ActorID: member, AgentSubjectID: id, ExpectedRevision: 1,
		Visibility: AgentVisibilityOwner, AddPolicy: AgentAddPolicyOwnerOnly,
		Entries: []AgentAccessEntry{{PrincipalID: member, Role: AgentAccessManager}},
	})
	roomErr := wantRoomErr(t, err, 422, CodeAgentPolicyInvalid)
	if roomErr.Details["field"] != "visibility" {
		t.Fatalf("details.field = %v, want visibility", roomErr.Details["field"])
	}
}

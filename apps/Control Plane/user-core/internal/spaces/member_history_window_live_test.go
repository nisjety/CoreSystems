package spaces

import (
	"context"
	"errors"
	"testing"
	"time"
)

// D18 live proofs (gated on TEST_DATABASE_URL through roomLiveFixture; skipped without it). They cover the
// invite's don't-share choice, the admin change and its role rule, the membership feed and the thread-read
// decision honouring a since_date window, the invite reset after a re-add, and the canvas refusal of a
// per-member timestamp floor. Every change writes its user_audit_outbox row in the same transaction.

// memberWindowFixture prepares a managed room with its owner, manager, editor and viewer, the history
// policy, the per-member switch on, the audit outbox wired, and the first audience snapshot.
func (f *roomLiveFixture) memberWindowFixture(t *testing.T, name string) roomStaff {
	t.Helper()
	s := f.staffedRoom(t, name)
	f.historyPolicy(t)
	f.repo.SetMemberHistoryWindows(true)
	f.repo.SetPolicyAuditOutbox(&txAuditOutbox{})
	f.publishAudience(t, s.ref, 1, s.owner, s.manager, s.viewer, s.editor)
	return s
}

// auditRows counts the user_audit_outbox rows with one event id.
func (f *roomLiveFixture) auditRows(t *testing.T, eventID string) int {
	t.Helper()
	var n int
	if err := f.pool.QueryRow(context.Background(),
		`SELECT count(*) FROM user_audit_outbox WHERE event_id=$1`, eventID).Scan(&n); err != nil {
		t.Fatalf("count audit rows %s: %v", eventID, err)
	}
	return n
}

func TestInviteNotSharedJoinsWithoutThePastLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	s := f.memberWindowFixture(t, "d18invite")
	ctx := context.Background()
	joiner := f.user(t, "d18invite-joiner", f.org)
	notShared := false

	result, err := f.repo.AddMember(ctx, MemberOpRequest{
		SpaceRef: s.ref, OrgID: f.org, ActorID: s.manager, SubjectID: joiner, Role: RoleViewer,
		SharePastConversation: &notShared,
	})
	if err != nil {
		t.Fatalf("add with don't-share: %v", err)
	}
	if result.HistoryWindow == nil || result.HistoryWindow.Source != historyFloorSourceMember ||
		result.HistoryWindow.Mode != MemberHistoryModeSinceJoin || result.HistoryWindow.Revision != 1 {
		t.Fatalf("the invite must answer a since_join member window: %+v", result.HistoryWindow)
	}
	if n := f.auditRows(t, memberHistoryWindowEventID(s.ref, joiner, 1)); n != 1 {
		t.Fatalf("the invite's window must write one audit row, found %d", n)
	}

	// Rev 2 is the snapshot in which the joiner first appears: their floor is that revision, and the
	// instant is their join (the ledger), so the signed thread read carries both.
	f.publishAudience(t, s.ref, 2, s.owner, s.manager, s.viewer, s.editor, joiner)
	evidence, err := f.repo.ResolveSharedThreadReadDecisionEvidence(ctx, s.ref, f.org, joiner)
	if err != nil {
		t.Fatalf("resolve the joiner's thread read: %v", err)
	}
	if evidence.HistoryFloorRevision != 2 || evidence.HistoryFloorAt == nil {
		t.Fatalf("the joiner must carry revision 2 and a join instant: revision=%d at=%v",
			evidence.HistoryFloorRevision, evidence.HistoryFloorAt)
	}
}

func TestInviteSharedResetsAStaleWindowOnReAddLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	s := f.memberWindowFixture(t, "d18reset")
	ctx := context.Background()
	joiner := f.user(t, "d18reset-joiner", f.org)
	notShared := false
	shared := true

	if _, err := f.repo.AddMember(ctx, MemberOpRequest{
		SpaceRef: s.ref, OrgID: f.org, ActorID: s.manager, SubjectID: joiner, Role: RoleViewer,
		SharePastConversation: &notShared,
	}); err != nil {
		t.Fatalf("first add (don't share): %v", err)
	}
	if _, err := f.repo.RemoveMember(ctx, f.op(s.ref, s.manager, joiner, "")); err != nil {
		t.Fatalf("remove: %v", err)
	}
	// Re-added with the default (share): the stale since_join row resets to the room's own mode.
	result, err := f.repo.AddMember(ctx, MemberOpRequest{
		SpaceRef: s.ref, OrgID: f.org, ActorID: s.manager, SubjectID: joiner, Role: RoleViewer,
		SharePastConversation: &shared,
	})
	if err != nil {
		t.Fatalf("re-add (share): %v", err)
	}
	if result.HistoryWindow == nil || result.HistoryWindow.Source != historyFloorSourceRoom || result.HistoryWindow.Revision != 2 {
		t.Fatalf("a shared re-add reads the room again at revision 2: %+v", result.HistoryWindow)
	}
	if n := f.auditRows(t, memberHistoryWindowEventID(s.ref, joiner, 2)); n != 1 {
		t.Fatalf("the reset must write its own audit row, found %d", n)
	}
}

func TestAdminChangesOneMemberWindowAndViewersAreRefusedLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	s := f.memberWindowFixture(t, "d18admin")
	ctx := context.Background()
	since := time.Now().UTC().Add(-48 * time.Hour).Truncate(time.Microsecond)

	// A viewer is not a room admin: refused, and nothing is written.
	_, err := f.repo.SetMemberHistoryWindow(ctx, MemberHistoryWindowRequest{
		SpaceRef: s.ref, OrgID: f.org, ActorID: s.viewer, SubjectID: s.editor, Mode: "full",
	})
	assertRoomCode(t, err, 403, CodeRoomRoleForbidden)

	// A manager moves the editor to history from a chosen date.
	result, err := f.repo.SetMemberHistoryWindow(ctx, MemberHistoryWindowRequest{
		SpaceRef: s.ref, OrgID: f.org, ActorID: s.manager, SubjectID: s.editor, Mode: "since_date", SinceAt: &since,
	})
	if err != nil {
		t.Fatalf("manager sets since_date: %v", err)
	}
	if !result.Changed || result.Window.Revision != 1 || result.Window.Mode != MemberHistoryModeSinceDate ||
		result.Window.SinceAt == nil || !result.Window.SinceAt.Equal(since) {
		t.Fatalf("since_date change: %+v", result)
	}
	if n := f.auditRows(t, memberHistoryWindowEventID(s.ref, s.editor, 1)); n != 1 {
		t.Fatalf("the change must write one audit row, found %d", n)
	}

	// The same window again is set-to-state: nothing changes.
	again, err := f.repo.SetMemberHistoryWindow(ctx, MemberHistoryWindowRequest{
		SpaceRef: s.ref, OrgID: f.org, ActorID: s.manager, SubjectID: s.editor, Mode: "since_date", SinceAt: &since,
	})
	if err != nil || again.Changed || again.Window.Revision != 1 {
		t.Fatalf("an unchanged window must not write: %+v %v", again, err)
	}

	// A stale expected revision is refused.
	stale := int64(0)
	_, err = f.repo.SetMemberHistoryWindow(ctx, MemberHistoryWindowRequest{
		SpaceRef: s.ref, OrgID: f.org, ActorID: s.manager, SubjectID: s.editor, Mode: "full", ExpectedRevision: &stale,
	})
	assertRoomCode(t, err, 409, CodeRoomMemberChanged)

	// The read answers the stored window to a manager, and refuses a viewer.
	read, err := f.repo.MemberHistoryWindowFor(ctx, s.ref, f.org, s.manager, s.editor)
	if err != nil || read.Window.Mode != MemberHistoryModeSinceDate || read.Window.Revision != 1 {
		t.Fatalf("manager read: %+v %v", read, err)
	}
	_, err = f.repo.MemberHistoryWindowFor(ctx, s.ref, f.org, s.viewer, s.editor)
	assertRoomCode(t, err, 403, CodeRoomRoleForbidden)
}

func TestSinceDateWindowHidesOlderMembershipRowsAndSignsTheInstantLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	s := f.memberWindowFixture(t, "d18date")
	ctx := context.Background()
	cut := time.Now().UTC().Add(-time.Hour).Truncate(time.Microsecond)
	older := "sme_d18_before_" + f.suffix
	newer := "sme_d18_after_" + f.suffix
	f.exec(t, `INSERT INTO space_membership_events
		(event_id, space_ref, org_id, space_seq, action, actor_kind, actor_id, created_at)
		VALUES ($1,$2,$3,9001,'member_added','user','d18-actor',$4)`,
		older, s.ref, f.org, cut.Add(-2*time.Hour))
	f.exec(t, `INSERT INTO space_membership_events
		(event_id, space_ref, org_id, space_seq, action, actor_kind, actor_id, created_at)
		VALUES ($1,$2,$3,9002,'member_added','user','d18-actor',NOW())`,
		newer, s.ref, f.org)

	if _, err := f.repo.SetMemberHistoryWindow(ctx, MemberHistoryWindowRequest{
		SpaceRef: s.ref, OrgID: f.org, ActorID: s.manager, SubjectID: s.editor, Mode: "since_date", SinceAt: &cut,
	}); err != nil {
		t.Fatalf("set since_date: %v", err)
	}

	feed, err := f.repo.MembershipFeed(ctx, s.ref, f.org, s.editor, MembershipFeedParams{Limit: MembershipFeedDefaultLimit})
	if err != nil {
		t.Fatalf("feed: %v", err)
	}
	seen := map[string]bool{}
	for _, e := range feed.Events {
		seen[e.EventID] = true
	}
	if !seen[newer] || seen[older] {
		t.Fatalf("since_date must show the row on/after the date and hide the older one: %+v", feed.Events)
	}

	evidence, err := f.repo.ResolveSharedThreadReadDecisionEvidence(ctx, s.ref, f.org, s.editor)
	if err != nil {
		t.Fatalf("resolve the editor's thread read: %v", err)
	}
	if evidence.HistoryFloorAt == nil || !evidence.HistoryFloorAt.Equal(cut) || evidence.HistoryFloorRevision != 0 {
		t.Fatalf("a since_date member carries the instant and no revision floor: at=%v revision=%d",
			evidence.HistoryFloorAt, evidence.HistoryFloorRevision)
	}
}

func TestCanvasRefusesAPerMemberTimestampFloorLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	s := f.memberWindowFixture(t, "d18canvas")
	ctx := context.Background()
	f.exec(t, `UPDATE space_effect_policies SET canvas_entitled=TRUE WHERE org_id=$1`, f.org)
	cut := time.Now().UTC().Add(-time.Hour).Truncate(time.Microsecond)
	if _, err := f.repo.SetMemberHistoryWindow(ctx, MemberHistoryWindowRequest{
		SpaceRef: s.ref, OrgID: f.org, ActorID: s.manager, SubjectID: s.editor, Mode: "since_date", SinceAt: &cut,
	}); err != nil {
		t.Fatalf("set since_date: %v", err)
	}
	_, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, false, CanvasOpRead)
	if !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
		t.Fatalf("a canvas read under a timestamp floor must be refused, not shown whole: %v", err)
	}
}

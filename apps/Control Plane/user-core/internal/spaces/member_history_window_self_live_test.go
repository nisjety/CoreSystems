package spaces

import (
	"context"
	"testing"
	"time"
)

// D18 self-read live proof (gated on TEST_DATABASE_URL through roomLiveFixture; skipped without it). A member's own
// membership read carries that member's window, derived from the same floor the reads sign. The member with a
// since_date row sees the date and that hiding is on; a member with no row sees the room's mode and nothing hidden.

func TestOwnHistoryWindowLiveIsTheCallersOwnRow(t *testing.T) {
	f := newRoomLiveFixture(t)
	s := f.memberWindowFixture(t, "d18self")
	ctx := context.Background()
	cut := time.Now().UTC().Add(-time.Hour).Truncate(time.Microsecond)
	if _, err := f.repo.SetMemberHistoryWindow(ctx, MemberHistoryWindowRequest{
		SpaceRef: s.ref, OrgID: f.org, ActorID: s.manager, SubjectID: s.editor, Mode: "since_date", SinceAt: &cut,
	}); err != nil {
		t.Fatalf("set since_date for the editor: %v", err)
	}

	editor, err := f.repo.ResolveReadableUserMembership(ctx, s.ref, f.org, s.editor)
	if err != nil {
		t.Fatalf("resolve the editor's membership: %v", err)
	}
	ownWindow, err := f.repo.OwnHistoryWindowFor(ctx, editor)
	if err != nil {
		t.Fatalf("the editor's own window: %v", err)
	}
	if ownWindow.Source != historyFloorSourceMember || ownWindow.Mode != MemberHistoryModeSinceDate ||
		ownWindow.SinceAt == nil || !ownWindow.SinceAt.Equal(cut) || !ownWindow.Hidden {
		t.Fatalf("the editor must read the since_date window they were given: %+v", ownWindow)
	}

	// The viewer has no row: the room's mode decides, and it hides nothing on its own.
	viewer, err := f.repo.ResolveReadableUserMembership(ctx, s.ref, f.org, s.viewer)
	if err != nil {
		t.Fatalf("resolve the viewer's membership: %v", err)
	}
	viewerWindow, err := f.repo.OwnHistoryWindowFor(ctx, viewer)
	if err != nil {
		t.Fatalf("the viewer's own window: %v", err)
	}
	if viewerWindow.Source != historyFloorSourceRoom || viewerWindow.Mode != f.historyMode(t, s.ref) ||
		viewerWindow.SinceAt != nil || viewerWindow.Hidden {
		t.Fatalf("a member with no row reads the room's mode and hides nothing: %+v", viewerWindow)
	}

	// The editor's own read must agree with the floor the thread read signs for them.
	evidence, err := f.repo.ResolveSharedThreadReadDecisionEvidence(ctx, s.ref, f.org, s.editor)
	if err != nil {
		t.Fatalf("resolve the editor's thread read: %v", err)
	}
	if evidence.HistoryFloorAt == nil || !evidence.HistoryFloorAt.Equal(*ownWindow.SinceAt) {
		t.Fatalf("the self window must match the floor the reads sign: read=%v self=%v", evidence.HistoryFloorAt, ownWindow.SinceAt)
	}
}

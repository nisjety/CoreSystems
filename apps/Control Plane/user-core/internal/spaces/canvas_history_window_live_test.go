package spaces

import (
	"context"
	"errors"
	"testing"
	"time"
)

// D18 canvas live proof (gated on TEST_DATABASE_URL through roomLiveFixture; skipped without it). With the canvas
// window on, a windowed member's canvas read is issued with the floor; a save is refused under it, and a create is
// allowed. With the window off, the read is refused as it was before this slice.

func TestCanvasHistoryWindowLiveIssuesTheReadAndRefusesASave(t *testing.T) {
	f := newRoomLiveFixture(t)
	s := f.memberWindowFixture(t, "d18canvaswindow")
	ctx := context.Background()
	f.exec(t, `UPDATE space_effect_policies SET canvas_entitled=TRUE WHERE org_id=$1`, f.org)
	cut := time.Now().UTC().Add(-time.Hour).Truncate(time.Microsecond)
	if _, err := f.repo.SetMemberHistoryWindow(ctx, MemberHistoryWindowRequest{
		SpaceRef: s.ref, OrgID: f.org, ActorID: s.manager, SubjectID: s.editor, Mode: "since_date", SinceAt: &cut,
	}); err != nil {
		t.Fatalf("set since_date: %v", err)
	}
	now := time.Now().UTC()

	// Window off (the default): the read is refused, as before this slice.
	if _, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, false); !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
		t.Fatalf("with SPACE_CANVAS_HISTORY_WINDOW off a windowed read must be refused: %v", err)
	}

	f.repo.SetCanvasHistoryWindow(true)
	defer f.repo.SetCanvasHistoryWindow(false)

	// Window on: the read resolves, carries the instant, and is issued for the canvas verifier to window.
	readEvidence, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, false)
	if err != nil {
		t.Fatalf("with the canvas window on, the read must resolve: %v", err)
	}
	if readEvidence.HistoryFloorAt == nil || !readEvidence.HistoryFloorAt.Equal(cut) {
		t.Fatalf("the canvas read evidence must carry the member's instant, got %v", readEvidence.HistoryFloorAt)
	}
	read, err := IssueCanvasDecision(readEvidence, canvasReadRequest(), now)
	if err != nil {
		t.Fatalf("issue the windowed canvas read: %v", err)
	}
	if read.HistoryFloorAt == nil || !read.HistoryFloorAt.Equal(cut) {
		t.Fatalf("the issued canvas read must sign the instant, got %v", read.HistoryFloorAt)
	}

	// A save under the floor is refused even with the window on; a create writes only new content and is allowed.
	writeEvidence, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, true)
	if err != nil {
		t.Fatalf("resolve the windowed editor's canvas write: %v", err)
	}
	if _, err := IssueCanvasDecision(writeEvidence, canvasSaveRequest(), now); !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
		t.Fatalf("a save under a floor must be refused, got %v", err)
	}
	if _, err := IssueCanvasDecision(writeEvidence, canvasCreateRequest(), now); err != nil {
		t.Fatalf("a create under a floor must be issued: %v", err)
	}
}

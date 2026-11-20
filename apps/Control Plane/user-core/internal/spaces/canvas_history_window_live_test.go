package spaces

import (
	"context"
	"errors"
	"testing"
	"time"
)

// D18/D21m canvas live proof (gated on TEST_DATABASE_URL through roomLiveFixture; skipped
// without it). With the canvas window on, a windowed member's canvas read AND save are
// issued with the floor, carried for wiki-store to window; a restore stays refused under
// the floor; a create is allowed (and carries the floor too). With the window off, a read
// or a save under the floor is refused, as a read alone was before D21m.

func TestCanvasHistoryWindowLiveIssuesTheReadAndTheSaveAndRefusesARestore(t *testing.T) {
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

	// Window off (the default): the read and the save are both refused, as a read alone was before D21m.
	if _, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, false, CanvasOpRead); !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
		t.Fatalf("with SPACE_CANVAS_HISTORY_WINDOW off a windowed read must be refused: %v", err)
	}
	if _, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, true, CanvasOpSave); !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
		t.Fatalf("with SPACE_CANVAS_HISTORY_WINDOW off a windowed save must be refused: %v", err)
	}

	f.repo.SetCanvasHistoryWindow(true)
	defer f.repo.SetCanvasHistoryWindow(false)

	// Window on: the read resolves, carries the instant, and is issued for the canvas verifier to window.
	readEvidence, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, false, CanvasOpRead)
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

	// Window on: a save now resolves and is issued too (D21m — the current canvas is current state, not
	// history), carrying the same instant for wiki-store to window its own conflict/unchanged answer.
	saveEvidence, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, true, CanvasOpSave)
	if err != nil {
		t.Fatalf("with the canvas window on, the save must resolve: %v", err)
	}
	if saveEvidence.HistoryFloorAt == nil || !saveEvidence.HistoryFloorAt.Equal(cut) {
		t.Fatalf("the canvas save evidence must carry the member's instant, got %v", saveEvidence.HistoryFloorAt)
	}
	save, err := IssueCanvasDecision(saveEvidence, canvasSaveRequest(), now)
	if err != nil {
		t.Fatalf("issue the windowed canvas save: %v", err)
	}
	if save.HistoryFloorAt == nil || !save.HistoryFloorAt.Equal(cut) {
		t.Fatalf("the issued canvas save must sign the instant, got %v", save.HistoryFloorAt)
	}

	// A restore stays refused under the floor regardless of the window switch: D21m hides every version
	// from before the window, and a create writes only new content either way.
	restoreEvidence, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, true, CanvasOpRestore)
	if err != nil {
		t.Fatalf("resolve the windowed editor's canvas restore: %v", err)
	}
	if _, err := IssueCanvasDecision(restoreEvidence, canvasRestoreRequest(), now); !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
		t.Fatalf("a restore under a floor must be refused, got %v", err)
	}
	createEvidence, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, true, CanvasOpCreate)
	if err != nil {
		t.Fatalf("resolve the windowed editor's canvas create: %v", err)
	}
	if _, err := IssueCanvasDecision(createEvidence, canvasCreateRequest(), now); err != nil {
		t.Fatalf("a create under a floor must be issued: %v", err)
	}
}

// TestCanvasHistoryWindowLiveSaveRevisionFloorStillRefusesWithWindowOn proves D21m touches only the D18
// timestamp floor: the older WS04-S11 revision floor still refuses a save even with the canvas window on.
func TestCanvasHistoryWindowLiveSaveRevisionFloorStillRefusesWithWindowOn(t *testing.T) {
	f := newRoomLiveFixture(t)
	s := f.archiveFixtureRoom(t, "d18canvasrevfloor")
	ctx := context.Background()
	f.exec(t, `UPDATE space_effect_policies SET canvas_entitled=TRUE WHERE org_id=$1`, f.org)
	f.repo.SetCanvasHistoryWindow(true)
	defer f.repo.SetCanvasHistoryWindow(false)

	evidence, err := f.repo.ResolveSharedCanvasDecisionEvidence(ctx, s.ref, f.org, s.editor, true, CanvasOpSave)
	if err != nil {
		t.Fatalf("resolve the unfloored editor's canvas save: %v", err)
	}
	evidence.HistoryFloorRevision = 3
	if _, err := IssueCanvasDecision(evidence, canvasSaveRequest(), time.Now().UTC()); !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
		t.Fatalf("a save under a revision floor must still be refused, got %v", err)
	}
}

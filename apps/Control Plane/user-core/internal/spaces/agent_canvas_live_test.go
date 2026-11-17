package spaces

import (
	"context"
	"fmt"
	"testing"
)

// W3-FIX-12 live proof (TEST_DATABASE_URL through roomLiveFixture): the agent
// evidence resolver reads canvas_entitled for a shared room, and the canvas
// run-action floor follows history_mode, the canvas bit and the
// SPACE_HISTORY_WINDOW_CLAIM switch.

// publishCanvasAudience writes one recipient-audience snapshot and makes it
// current. The ref and hash are built here so each SQL parameter has one type.
func (f *roomLiveFixture) publishCanvasAudience(t *testing.T, ref string, revision int64, subjects ...string) {
	t.Helper()
	f.exec(t, `INSERT INTO space_recipient_audiences (space_ref, revision, audience_ref, audience_hash, recipient_count)
		VALUES ($1, $2, $3, $4, $5)`, ref, revision, fmt.Sprintf("audience:%s:%d", ref, revision), fmt.Sprintf("sha256:w3fix12-%d", revision), len(subjects))
	for _, subject := range subjects {
		f.exec(t, `INSERT INTO space_recipient_audience_members (space_ref, revision, subject_id) VALUES ($1, $2, $3)`, ref, revision, subject)
	}
	f.exec(t, `UPDATE space_authority_revisions SET recipient_audience_revision=$2 WHERE space_ref=$1`, ref, revision)
}

func TestAgentActionEvidenceReadsTheCanvasBitLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	ctx := context.Background()
	s := f.archiveFixtureRoom(t, "agentcanvas")

	for _, canvas := range []bool{true, false} {
		f.exec(t, `UPDATE space_effect_policies SET canvas_entitled=$2 WHERE org_id=$1`, f.org, canvas)
		evidence, err := f.repo.ResolveAgentActionDecisionEvidence(ctx, s.ref, f.org, s.editor)
		if err != nil {
			t.Fatalf("resolve agent action evidence (canvas %v): %v", canvas, err)
		}
		if evidence.Membership.Kind != KindRoom || evidence.CanvasEntitled != canvas {
			t.Fatalf("agent evidence kind %q canvas %v, want room and %v", evidence.Membership.Kind, evidence.CanvasEntitled, canvas)
		}
	}
}

func TestCanvasRunActionFloorFollowsHistoryModeAndSwitchLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	ctx := context.Background()
	s := f.archiveFixtureRoom(t, "canvasfloor")
	f.exec(t, `UPDATE space_effect_policies SET canvas_entitled=TRUE WHERE org_id=$1`, f.org)
	// Revision 2 leaves the editor out and revision 3 brings them back, so the
	// editor's floor is 3; the owner is in every snapshot and has none.
	f.publishCanvasAudience(t, s.ref, 2, s.owner, s.manager, s.viewer)
	f.publishCanvasAudience(t, s.ref, 3, s.owner, s.manager, s.editor, s.viewer)
	f.exec(t, `UPDATE registered_spaces SET history_mode='since_join' WHERE space_ref=$1`, s.ref)

	floor := func(t *testing.T, subject string) int64 {
		t.Helper()
		evidence, err := f.repo.ResolveAgentActionDecisionEvidence(ctx, s.ref, f.org, subject)
		if err != nil {
			t.Fatalf("resolve agent action evidence for %s: %v", subject, err)
		}
		if err := f.repo.ApplyCanvasRunActionFloor(ctx, &evidence); err != nil {
			t.Fatalf("ApplyCanvasRunActionFloor for %s: %v", subject, err)
		}
		return evidence.HistoryFloorRevision
	}

	f.repo.SetHistoryWindowClaim(false)
	if got := floor(t, s.editor); got != 0 {
		t.Fatalf("switch off must set no floor, got %d", got)
	}
	f.repo.SetHistoryWindowClaim(true)
	if got := floor(t, s.editor); got != 3 {
		t.Fatalf("a since_join editor who rejoined at revision 3 must get floor 3, got %d", got)
	}
	if got := floor(t, s.owner); got != 0 {
		t.Fatalf("the owner was in every snapshot: no floor expected, got %d", got)
	}

	// Canvas off: the floor is not read at all.
	f.exec(t, `UPDATE space_effect_policies SET canvas_entitled=FALSE WHERE org_id=$1`, f.org)
	if got := floor(t, s.editor); got != 0 {
		t.Fatalf("canvas off must set no floor, got %d", got)
	}
	// A full-history room has no floor.
	f.exec(t, `UPDATE space_effect_policies SET canvas_entitled=TRUE WHERE org_id=$1`, f.org)
	f.exec(t, `UPDATE registered_spaces SET history_mode='full' WHERE space_ref=$1`, s.ref)
	if got := floor(t, s.editor); got != 0 {
		t.Fatalf("history_mode full must set no floor, got %d", got)
	}
}

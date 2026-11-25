package spaces

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// R-sws-01 live leg: the run-control resolver binds the since_join history floor that the thread-read
// resolver binds, so a manager who joined a since_join room after a run started gets a stop decision that
// Session Core can refuse for that run. Gated on TEST_DATABASE_URL through roomLiveFixture.
func TestRunControlFloorOnALateJoinerLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	f.historyPolicy(t)
	f.exec(t, `UPDATE space_effect_policies SET run_control_entitled=TRUE WHERE org_id=$1`, f.org)
	s := f.staffedRoom(t, "runfloor")
	f.exec(t, `UPDATE registered_spaces SET history_mode='since_join' WHERE space_ref=$1`, s.ref)
	f.repo.SetHistoryWindowClaim(true)

	// rev 1: the manager is not yet in the room. rev 2: the manager joins, so their floor is 2. The owner
	// sat in every snapshot and has no floor.
	f.publishAudience(t, s.ref, 1, s.owner, s.viewer, s.editor)
	f.publishAudience(t, s.ref, 2, s.owner, s.viewer, s.editor, s.manager)

	stopDecision := func(subject string) (PersonalThreadDecisionEvidence, Decision) {
		t.Helper()
		evidence, err := f.repo.ResolveSharedRunControlDecisionEvidence(ctx, s.ref, f.org, subject)
		if err != nil {
			t.Fatalf("resolve run-control evidence for %s: %v", subject, err)
		}
		decision, err := IssueSharedRunControlDecision(evidence, validRunControlRequest(), time.Now().UTC())
		if err != nil {
			t.Fatalf("issue run-control decision for %s: %v", subject, err)
		}
		return evidence, decision
	}

	// The late joiner carries the floor on the evidence and on the signed decision.
	evidence, decision := stopDecision(s.manager)
	if evidence.HistoryFloorRevision != 2 || decision.HistoryFloorRevision != 2 {
		t.Fatalf("manager added at revision 2: evidence floor %d, decision floor %d", evidence.HistoryFloorRevision, decision.HistoryFloorRevision)
	}
	raw, err := json.Marshal(decision)
	if err != nil {
		t.Fatalf("marshal run-control decision: %v", err)
	}
	if !strings.Contains(string(raw), `"history_floor_revision":2`) {
		t.Fatalf("the run-control decision must sign the floor: %s", raw)
	}

	// The owner was in every snapshot: no floor, and the claim is absent from the decision.
	_, ownerDecision := stopDecision(s.owner)
	if ownerDecision.HistoryFloorRevision != 0 {
		t.Fatalf("a member present in every snapshot has no stop floor, got %d", ownerDecision.HistoryFloorRevision)
	}
	raw, err = json.Marshal(ownerDecision)
	if err != nil {
		t.Fatalf("marshal owner run-control decision: %v", err)
	}
	if strings.Contains(string(raw), "history_floor_revision") {
		t.Fatalf("a run-control decision without a floor must omit the claim: %s", raw)
	}

	// history_mode full: the late joiner's stop decision carries no floor.
	f.exec(t, `UPDATE registered_spaces SET history_mode='full' WHERE space_ref=$1`, s.ref)
	if _, full := stopDecision(s.manager); full.HistoryFloorRevision != 0 {
		t.Fatalf("history_mode full must emit no stop floor, got %d", full.HistoryFloorRevision)
	}

	// since_join with the switch off: no stop floor either.
	f.exec(t, `UPDATE registered_spaces SET history_mode='since_join' WHERE space_ref=$1`, s.ref)
	f.repo.SetHistoryWindowClaim(false)
	if _, off := stopDecision(s.manager); off.HistoryFloorRevision != 0 {
		t.Fatalf("flag off must emit no stop floor, got %d", off.HistoryFloorRevision)
	}
}

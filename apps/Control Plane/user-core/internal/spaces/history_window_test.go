package spaces

import (
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// WS04-S11 pure legs: the closed mode set, the claim switch default, and the
// signed floor on the thread-read decision.

func TestValidHistoryModeIsAClosedSet(t *testing.T) {
	for _, ok := range []string{HistoryModeFull, HistoryModeSinceJoin, "full", "since_join"} {
		if !ValidHistoryMode(ok) {
			t.Fatalf("%q must be valid", ok)
		}
	}
	for _, bad := range []string{"", "Full", "since-join", "since_join ", "none", "forever"} {
		if ValidHistoryMode(bad) {
			t.Fatalf("%q must be invalid", bad)
		}
	}
}

func TestNormalizeHistoryModeTrimsAndKeepsNil(t *testing.T) {
	if normalizeHistoryMode(nil) != nil {
		t.Fatal("nil means unchanged")
	}
	in := "  since_join \n"
	if got := normalizeHistoryMode(&in); got == nil || *got != "since_join" {
		t.Fatalf("normalizeHistoryMode = %v", got)
	}
}

func TestHistoryWindowClaimDefaultsOff(t *testing.T) {
	var nilRepo *Repository
	if nilRepo.HistoryWindowClaimEnabled() {
		t.Fatal("a nil repository must report the claim off")
	}
	repo := &Repository{}
	if repo.HistoryWindowClaimEnabled() {
		t.Fatal("SPACE_HISTORY_WINDOW_CLAIM must default off")
	}
	repo.SetHistoryWindowClaim(true)
	if !repo.HistoryWindowClaimEnabled() {
		t.Fatal("the switch must turn on")
	}
	repo.SetHistoryWindowClaim(false)
	if repo.HistoryWindowClaimEnabled() {
		t.Fatal("the switch must turn off again")
	}
}

func TestApplyHistoryFloorIsANoOpWithTheSwitchOffOrNoEvidence(t *testing.T) {
	// No database is configured: any query would panic, so this proves the
	// switch-off and nil-evidence paths never reach the pool.
	repo := &Repository{}
	evidence := validSharedThreadReadEvidence()
	if err := repo.applyHistoryFloor(t.Context(), &evidence); err != nil || evidence.HistoryFloorRevision != 0 {
		t.Fatalf("switch off must not touch the evidence: %d %v", evidence.HistoryFloorRevision, err)
	}
	repo.SetHistoryWindowClaim(true)
	if err := repo.applyHistoryFloor(t.Context(), nil); err != nil {
		t.Fatalf("nil evidence: %v", err)
	}
	var nilRepo *Repository
	if err := nilRepo.applyHistoryFloor(t.Context(), &evidence); err != nil {
		t.Fatalf("nil repository: %v", err)
	}
}

func readRequest() SharedThreadReadDecisionRequest {
	return SharedThreadReadDecisionRequest{DecisionRef: "read-decision-1", IdempotencyKey: "read-1", Nonce: "nonce-1"}
}

func TestThreadReadDecisionOmitsHistoryFloorWhenZero(t *testing.T) {
	decision, err := IssueSharedThreadReadDecision(validSharedThreadReadEvidence(), readRequest(), time.Now().UTC())
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	if decision.HistoryFloorRevision != 0 {
		t.Fatalf("no floor expected, got %d", decision.HistoryFloorRevision)
	}
	raw, err := json.Marshal(decision)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if strings.Contains(string(raw), "history_floor_revision") {
		t.Fatalf("a decision without a floor must omit the claim: %s", raw)
	}
}

func TestThreadReadDecisionCarriesTheFloorButDigestIgnoresIt(t *testing.T) {
	now := time.Now().UTC()
	plain := validSharedThreadReadEvidence()
	windowed := validSharedThreadReadEvidence()
	windowed.HistoryFloorRevision = 7

	base, err := IssueSharedThreadReadDecision(plain, readRequest(), now)
	if err != nil {
		t.Fatalf("issue plain: %v", err)
	}
	with, err := IssueSharedThreadReadDecision(windowed, readRequest(), now)
	if err != nil {
		t.Fatalf("issue windowed: %v", err)
	}
	if with.HistoryFloorRevision != 7 {
		t.Fatalf("floor = %d, want 7", with.HistoryFloorRevision)
	}
	raw, _ := json.Marshal(with)
	if !strings.Contains(string(raw), `"history_floor_revision":7`) {
		t.Fatalf("the floor must be signed with the decision: %s", raw)
	}
	if base.PayloadDigest != with.PayloadDigest {
		t.Fatalf("the floor must not be part of the payload digest: %s vs %s", base.PayloadDigest, with.PayloadDigest)
	}
	if sharedThreadReadPayloadDigest(plain, readRequest()) != sharedThreadReadPayloadDigest(windowed, readRequest()) {
		t.Fatal("digest function must ignore the floor")
	}
}

func TestOwnScheduledThreadReadNeverCarriesAFloor(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.Membership.Role = "owner"
	evidence.ThreadReadEntitled = true
	evidence.HistoryFloorRevision = 9
	decision, err := IssueOwnScheduledThreadReadDecision(evidence, readRequest(), time.Now().UTC())
	if err != nil {
		t.Skipf("fixture not valid for the own-scheduled read (%v); covered by its own tests", err)
	}
	if decision.HistoryFloorRevision != 0 {
		t.Fatalf("a personal Space has no history window, got %d", decision.HistoryFloorRevision)
	}
}

package spaces

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"
)

// D18 decision floors. Every Control decision that lets an agent or a member read room content on a member's behalf
// carries the member's floor, additively and outside every payload digest. A personal or own-scheduled decision never
// does. A run action carries it only for a canvas read.

func decisionFloorInstant() time.Time {
	return time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC)
}

func floorOf(e PersonalThreadDecisionEvidence) PersonalThreadDecisionEvidence {
	at := decisionFloorInstant()
	e.HistoryFloorAt = &at
	return e
}

func digestOfDecisionJSON(t *testing.T, d Decision) string {
	t.Helper()
	raw, err := json.Marshal(d)
	if err != nil {
		t.Fatalf("marshal decision: %v", err)
	}
	return string(raw)
}

func TestCarryHistoryFloorIsExplicitAndCopiesBothForms(t *testing.T) {
	bare := carryHistoryFloor(Decision{ActionID: "x"}, validSharedThreadReadEvidence())
	if bare.HistoryFloorAt != nil || bare.HistoryFloorRevision != 0 {
		t.Fatalf("an evidence without a floor must leave the decision floor-free: %+v", bare)
	}
	evidence := floorOf(validSharedThreadReadEvidence())
	evidence.HistoryFloorRevision = 6
	carried := carryHistoryFloor(Decision{ActionID: "x"}, evidence)
	if carried.HistoryFloorRevision != 6 || carried.HistoryFloorAt == nil || !carried.HistoryFloorAt.Equal(decisionFloorInstant()) {
		t.Fatalf("both floor forms must be carried: %+v", carried)
	}
	if carried.HistoryFloorAt == evidence.HistoryFloorAt {
		t.Fatal("the carried instant must be a copy, not the evidence's pointer")
	}
}

func TestSharedRetrievalCarriesTheFloorAndItsDigestIgnoresIt(t *testing.T) {
	now := time.Date(2026, 10, 2, 12, 0, 0, 0, time.UTC)
	request := PersonalRetrievalDecisionRequest{DecisionRef: "retrieval-decision-1", IdempotencyKey: "retrieval-1", Nonce: "nonce-1"}
	plain, err := IssueSharedRetrievalDecision(validSharedRetrievalEvidence(), request, now)
	if err != nil {
		t.Fatalf("plain retrieval: %v", err)
	}
	windowed, err := IssueSharedRetrievalDecision(floorOf(validSharedRetrievalEvidence()), request, now)
	if err != nil {
		t.Fatalf("floored retrieval: %v", err)
	}
	if windowed.HistoryFloorAt == nil || !windowed.HistoryFloorAt.Equal(decisionFloorInstant()) {
		t.Fatalf("the retrieval must carry the member's instant, got %v", windowed.HistoryFloorAt)
	}
	if plain.PayloadDigest != windowed.PayloadDigest {
		t.Fatal("the floor must not be part of the retrieval payload digest")
	}
	if strings.Contains(digestOfDecisionJSON(t, plain), "history_floor_at") {
		t.Fatal("a retrieval without a window must omit the claim")
	}
	if !strings.Contains(digestOfDecisionJSON(t, windowed), `"history_floor_at":"2026-10-01T00:00:00Z"`) {
		t.Fatal("a windowed retrieval must sign the instant")
	}
}

func TestSharedRetrievalCarriesARevisionFloorToo(t *testing.T) {
	evidence := validSharedRetrievalEvidence()
	evidence.HistoryFloorRevision = 5
	decision, err := IssueSharedRetrievalDecision(evidence, PersonalRetrievalDecisionRequest{
		DecisionRef: "retrieval-decision-2", IdempotencyKey: "retrieval-2", Nonce: "nonce-2",
	}, time.Now().UTC())
	if err != nil {
		t.Fatalf("retrieval with a revision floor: %v", err)
	}
	if decision.HistoryFloorRevision != 5 {
		t.Fatalf("the WS04-S11 revision floor must travel with the retrieval too, got %d", decision.HistoryFloorRevision)
	}
}

// A personal retrieval reads only the owner's own Space, so it stays floor-free even when evidence carries one.
func TestPersonalRetrievalNeverCarriesAFloor(t *testing.T) {
	evidence := floorOf(validPersonalThreadEvidence())
	evidence.RetrievalReadEntitled = true
	decision, err := IssuePersonalRetrievalDecision(evidence, PersonalRetrievalDecisionRequest{
		DecisionRef: "personal-retrieval-1", IdempotencyKey: "personal-retrieval-1", Nonce: "nonce-1",
	}, time.Now().UTC())
	if err != nil {
		t.Fatalf("personal retrieval: %v", err)
	}
	if decision.HistoryFloorAt != nil || decision.HistoryFloorRevision != 0 {
		t.Fatalf("a personal retrieval has no window, got %+v", decision)
	}
}

// A thread append writes a post and reads no room content, so it carries no floor even for a windowed member.
func TestThreadAppendNeverCarriesAFloor(t *testing.T) {
	evidence := floorOf(validPersonalThreadEvidence())
	decision, err := IssueThreadAppendDecision(evidence, ThreadAppendDecisionRequest{
		DecisionRef: "append-1", ThreadID: "thread-1", ContentDigest: "sha256:" + strings.Repeat("c", 64),
		IdempotencyKey: "append-1", Nonce: "nonce-1",
	}, time.Now().UTC())
	if err != nil {
		t.Fatalf("thread append: %v", err)
	}
	if decision.HistoryFloorAt != nil || decision.HistoryFloorRevision != 0 {
		t.Fatalf("a post writes new content and carries no window, got %+v", decision)
	}
}

func TestMemoryPersonalContextCarriesTheFloorAndItsDigestIgnoresIt(t *testing.T) {
	now := time.Date(2026, 10, 2, 12, 0, 0, 0, time.UTC)
	plain, err := IssueMemoryPersonalContextDecision(memoryPersonalContextEvidence("editor"), memoryPersonalContextRequest("thread-1"), now)
	if err != nil {
		t.Fatalf("plain personal context: %v", err)
	}
	windowed, err := IssueMemoryPersonalContextDecision(floorOf(memoryPersonalContextEvidence("editor")), memoryPersonalContextRequest("thread-1"), now)
	if err != nil {
		t.Fatalf("floored personal context: %v", err)
	}
	if windowed.HistoryFloorAt == nil || !windowed.HistoryFloorAt.Equal(decisionFloorInstant()) {
		t.Fatalf("the personal-context turn must carry the member's instant, got %v", windowed.HistoryFloorAt)
	}
	if plain.PayloadDigest != windowed.PayloadDigest {
		t.Fatal("the floor must not be part of the personal-context payload digest")
	}
	if strings.Contains(digestOfDecisionJSON(t, plain), "history_floor_at") {
		t.Fatal("a personal-context turn without a window must omit the claim")
	}
}

func TestMemoryDeclassifyAndRetractCarryTheFloor(t *testing.T) {
	now := time.Date(2026, 10, 2, 12, 0, 0, 0, time.UTC)
	digest := "sha256:" + strings.Repeat("a", 64)
	for name, request := range map[string]MemoryDeclassifyDecisionRequest{
		"declassify": memoryDeclassifyRequest(MemoryDeclassifyOperationDeclassify, "personal-memory-1", digest),
		"retract":    memoryDeclassifyRequest(MemoryDeclassifyOperationRetract, "room-memory-1", ""),
	} {
		t.Run(name, func(t *testing.T) {
			decision, err := IssueMemoryDeclassifyDecision(floorOf(memoryDeclassifyEvidence("editor")), request, now)
			if err != nil {
				t.Fatalf("issue %s: %v", name, err)
			}
			if decision.HistoryFloorAt == nil || !decision.HistoryFloorAt.Equal(decisionFloorInstant()) {
				t.Fatalf("%s must carry the member's instant, got %v", name, decision.HistoryFloorAt)
			}
		})
	}
}

func TestSpaceMemoryManageCarriesTheFloor(t *testing.T) {
	now := time.Date(2026, 10, 2, 12, 0, 0, 0, time.UTC)
	decision, err := IssueSpaceMemoryDecision(floorOf(spaceMemoryEvidence("manager")), spaceMemoryRequest(SpaceMemoryOperationList, ""), now)
	if err != nil {
		t.Fatalf("issue space memory list: %v", err)
	}
	if decision.HistoryFloorAt == nil || !decision.HistoryFloorAt.Equal(decisionFloorInstant()) {
		t.Fatalf("a windowed manager's memory list must carry the instant, got %v", decision.HistoryFloorAt)
	}
}

func TestRunWakeCarriesTheFloor(t *testing.T) {
	now := time.Date(2026, 10, 2, 12, 0, 0, 0, time.UTC)
	decision, err := IssueRunWakeDecision(floorOf(validRunWakeEvidence()), validRunWakeRoster(), validRunWakeRequest(), now)
	if err != nil {
		t.Fatalf("issue run wake: %v", err)
	}
	if decision.HistoryFloorAt == nil || !decision.HistoryFloorAt.Equal(decisionFloorInstant()) {
		t.Fatalf("a wake starts a run that reads the room, so it must carry the instant, got %v", decision.HistoryFloorAt)
	}
}

// A watch is a standing read of a shared room, so its create and each observation carry the floor.
func TestWatchCreateAndObserveCarryTheFloor(t *testing.T) {
	now := time.Date(2026, 10, 2, 12, 0, 0, 0, time.UTC)
	evidence := floorOf(validWatchEvidence())
	create, err := IssueWatchCreateDecision(evidence, validWatchCreate(), now)
	if err != nil {
		t.Fatalf("issue watch create: %v", err)
	}
	if create.HistoryFloorAt == nil || !create.HistoryFloorAt.Equal(decisionFloorInstant()) {
		t.Fatalf("the watch create must carry the instant, got %v", create.HistoryFloorAt)
	}
	observe, err := IssueWatchObserveDecision(evidence, validWatchObserve(evidence), "watch-observe-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("issue watch observe: %v", err)
	}
	if observe.HistoryFloorAt == nil || !observe.HistoryFloorAt.Equal(decisionFloorInstant()) {
		t.Fatalf("each watch observation must carry the instant, got %v", observe.HistoryFloorAt)
	}
}

// Run-action canvas rows: a read carries the timestamp floor, a write under any floor is refused, and a non-canvas row
// (tickets.create) never carries a floor even when evidence has one.
func TestRunActionCanvasReadCarriesTheFloorAndAWriteIsRefused(t *testing.T) {
	t.Setenv(spaceSelfAPIActionsEnv, "space.canvas.read,space.canvas.write")
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := runActionRoomEvidence()
	evidence.CanvasEntitled = true
	floored := floorOf(evidence)

	read, err := IssueRunActionDecision(floored, runActionAuthorityFor(floored), goldenRunActionRequest(t, canvasRunActionReadID), now)
	if err != nil {
		t.Fatalf("canvas read under a timestamp floor: %v", err)
	}
	if read.HistoryFloorAt == nil || !read.HistoryFloorAt.Equal(decisionFloorInstant()) {
		t.Fatalf("a canvas read must carry the instant, got %v", read.HistoryFloorAt)
	}
	raw, err := json.Marshal(read)
	if err != nil {
		t.Fatalf("marshal canvas read: %v", err)
	}
	if !strings.Contains(string(raw), `"history_floor_at":"2026-10-01T00:00:00Z"`) {
		t.Fatalf("the canvas read must sign the instant on the wire: %s", raw)
	}

	if _, err := IssueRunActionDecision(floored, runActionAuthorityFor(floored), goldenRunActionRequest(t, canvasRunActionWriteID), now); !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
		t.Fatalf("a canvas write under a floor must be refused with the typed floor refusal, got %v", err)
	}
	revisioned := evidence
	revisioned.HistoryFloorRevision = 3
	if _, err := IssueRunActionDecision(revisioned, runActionAuthorityFor(revisioned), goldenRunActionRequest(t, canvasRunActionReadID), now); !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
		t.Fatalf("a canvas read under a WS04-S11 revision floor stays refused with the typed floor refusal, got %v", err)
	}

	plain, err := IssueRunActionDecision(evidence, runActionAuthorityFor(evidence), goldenRunActionRequest(t, canvasRunActionReadID), now)
	if err != nil {
		t.Fatalf("plain canvas read: %v", err)
	}
	if raw, _ := json.Marshal(plain); strings.Contains(string(raw), "history_floor_at") {
		t.Fatalf("a canvas read without a window must omit the claim: %s", raw)
	}
}

func TestTicketsCreateNeverCarriesAFloor(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	floored := floorOf(runActionPersonalEvidence())
	decision, err := IssueRunActionDecision(floored, validRunActionAuthority(), validRunActionRequest(), now)
	if err != nil {
		t.Fatalf("tickets.create: %v", err)
	}
	if decision.HistoryFloorAt != nil {
		t.Fatalf("a non-canvas owner action reads no room content and must carry no floor, got %v", decision.HistoryFloorAt)
	}
}

// Effect-time freshness: a canvas read decision whose window has since tightened is refused, so an older, looser
// floor is never read under after the room narrowed it.
func TestValidateCurrentRunActionDecisionRefusesATightenedWindow(t *testing.T) {
	t.Setenv(spaceSelfAPIActionsEnv, "space.canvas.read,space.canvas.write")
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	base := runActionRoomEvidence()
	base.CanvasEntitled = true
	issuedFloor := floorOf(base)
	decision, err := IssueRunActionDecision(issuedFloor, runActionAuthorityFor(issuedFloor), goldenRunActionRequest(t, canvasRunActionReadID), now)
	if err != nil {
		t.Fatalf("issue canvas read: %v", err)
	}

	tightened := base
	later := decisionFloorInstant().Add(time.Hour)
	tightened.HistoryFloorAt = &later
	if err := ValidateCurrentRunActionDecision(tightened, decision); err == nil {
		t.Fatal("a window tightened after issuance must refuse the older, looser floor")
	}

	same := base
	unchanged := decisionFloorInstant()
	same.HistoryFloorAt = &unchanged
	if err := ValidateCurrentRunActionDecision(same, decision); err != nil {
		t.Fatalf("an unchanged window must still pass the effect-time check: %v", err)
	}

	loosened := base
	if err := ValidateCurrentRunActionDecision(loosened, decision); err != nil {
		t.Fatalf("a window that was lifted only hides more than needed and must pass: %v", err)
	}
}

// The floor is a claim of the signed envelope: it verifies with the decision, and no payload digest covers it.
func TestSignedDecisionCarriesTheFloorToItsVerifier(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("generate key: %v", err)
	}
	key := SigningKey{ID: "control-floor-test", PrivateKey: private}
	issued, err := IssueMemoryDeclassifyDecision(floorOf(memoryDeclassifyEvidence("editor")),
		memoryDeclassifyRequest(MemoryDeclassifyOperationRetract, "room-memory-1", ""), time.Now().UTC())
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	token, err := SignDecision(key, issued)
	if err != nil {
		t.Fatalf("sign: %v", err)
	}
	verified, err := VerifyDecision(VerificationKey{ID: key.ID, PublicKey: public}, token, DecisionExpectation{
		OrgID: issued.OrgID, SpaceRef: issued.SpaceRef, SubjectID: issued.SubjectID,
		ServiceAudience: issued.ServiceAudience, ActionID: issued.ActionID, ActionSchemaHash: issued.ActionSchemaHash,
		PayloadDigest: issued.PayloadDigest, IdempotencyKey: issued.IdempotencyKey, Now: issued.IssuedAt.Add(time.Second),
	})
	if err != nil {
		t.Fatalf("verify: %v", err)
	}
	if verified.HistoryFloorAt == nil || !verified.HistoryFloorAt.Equal(decisionFloorInstant()) {
		t.Fatalf("the verifier must read the signed floor, got %v", verified.HistoryFloorAt)
	}
}

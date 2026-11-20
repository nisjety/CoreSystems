package spacedecision

import (
	"encoding/json"
	"testing"
	"time"
)

// signPayloadWith signs claims the way Control does, after setting one JSON field of the payload to value. It lets a test carry a
// value the Claims type would never produce, such as a malformed or a zero floor.
func signPayloadWith(t *testing.T, version string, signer testSigner, claims any, field string, value any) string {
	t.Helper()
	raw, err := json.Marshal(claims)
	if err != nil {
		t.Fatalf("marshal claims: %v", err)
	}
	var payload map[string]any
	if err := json.Unmarshal(raw, &payload); err != nil {
		t.Fatalf("unmarshal claims: %v", err)
	}
	payload[field] = value
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	return signRaw(version, signer, body)
}

// TestVerifyCarriesTheHistoryFloorInstant: a signed history_floor_at is read, so the canvas repository can window the read with it.
// A verifier that dropped it would show the whole version history.
func TestVerifyCarriesTheHistoryFloorInstant(t *testing.T) {
	signer := newTestSigner("floor-carry")
	keys := KeySet{signer.id: signer.public()}
	at := verifyNow.Add(-2 * time.Hour)
	claims := wellFormedClaims(verifyNow)
	claims.HistoryFloorRevision = 4
	claims.HistoryFloorAt = &at

	got, err := Verify(signEnvelope(t, EnvelopeVersion, signer, claims), keys, verifyNow)
	if err != nil {
		t.Fatalf("a decision with a floor was refused: %v", err)
	}
	if got.HistoryFloorAt == nil || !got.HistoryFloorAt.Equal(at) {
		t.Fatalf("history_floor_at = %v, want %v", got.HistoryFloorAt, at)
	}
	if got.HistoryFloorRevision != 4 {
		t.Fatalf("history_floor_revision = %d, want 4", got.HistoryFloorRevision)
	}
}

// TestVerifyLeavesAnAbsentFloorUnset: a decision that names no floor reads as unfloored, so the whole version history is shown to it.
func TestVerifyLeavesAnAbsentFloorUnset(t *testing.T) {
	signer := newTestSigner("floor-absent")
	keys := KeySet{signer.id: signer.public()}

	got, err := Verify(signEnvelope(t, EnvelopeVersion, signer, wellFormedClaims(verifyNow)), keys, verifyNow)
	if err != nil {
		t.Fatalf("a decision without a floor was refused: %v", err)
	}
	if got.HistoryFloorAt != nil || got.HistoryFloorRevision != 0 {
		t.Fatalf("an unfloored decision read as floored: at %v, revision %d", got.HistoryFloorAt, got.HistoryFloorRevision)
	}
}

// TestVerifyRefusesAMalformedOrEmptyHistoryFloor: a floor that is present but not a real instant is refused. It must not read as
// no floor, because a floor only narrows the read.
func TestVerifyRefusesAMalformedOrEmptyHistoryFloor(t *testing.T) {
	signer := newTestSigner("floor-malformed")
	keys := KeySet{signer.id: signer.public()}
	cases := map[string]any{
		"not a time":   "yesterday",
		"empty string": "",
		"zero instant": "0001-01-01T00:00:00Z",
	}
	for name, value := range cases {
		t.Run(name, func(t *testing.T) {
			token := signPayloadWith(t, EnvelopeVersion, signer, wellFormedClaims(verifyNow), "history_floor_at", value)
			_, err := Verify(token, keys, verifyNow)
			requireRefusal(t, err, CodeDecisionInvalid)
		})
	}
}

// TestVerifyRefusesANegativeRevisionFloor: a negative revision floor is not a window, so it is refused rather than read as none.
func TestVerifyRefusesANegativeRevisionFloor(t *testing.T) {
	signer := newTestSigner("floor-negative")
	keys := KeySet{signer.id: signer.public()}
	claims := wellFormedClaims(verifyNow)
	claims.HistoryFloorRevision = -1

	_, err := Verify(signEnvelope(t, EnvelopeVersion, signer, claims), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)
}

// TestCheckCanvasDoesNotRefuseAHistoryFloor: a floor is a window the repository applies to a read, and a handler refuses the writes it
// forbids. The binding check is not a refusal for either, so a windowed member's read reaches the repository.
func TestCheckCanvasDoesNotRefuseAHistoryFloor(t *testing.T) {
	now := verifyNow
	at := now.Add(-time.Hour)
	read := wellFormedClaims(now)
	read.HistoryFloorRevision = 3
	read.HistoryFloorAt = &at
	if err := read.CheckCanvas("org-1", "user-2", "space-room-1", false); err != nil {
		t.Fatalf("a floored read was refused by the binding check: %v", err)
	}
	write := wellFormedWriteClaims(now)
	write.HistoryFloorRevision = 3
	write.HistoryFloorAt = &at
	if err := write.CheckCanvas("org-1", "user-2", "space-room-1", true); err != nil {
		t.Fatalf("a floored write was refused by the binding check: %v", err)
	}
}

// TestVerifyRunActionCarriesTheHistoryFloorInstant: an agent's canvas read names the timestamp floor Control signs on it, so the
// agent reader windows the read too. Control never signs a revision floor on a run action.
func TestVerifyRunActionCarriesTheHistoryFloorInstant(t *testing.T) {
	signer := newTestSigner("run-floor")
	keys := KeySet{signer.id: signer.public()}
	at := verifyNow.Add(-time.Hour)
	run := wellFormedRunClaims(verifyNow)
	run.HistoryFloorAt = &at

	got, err := VerifyRunAction(signEnvelope(t, RunEnvelopeVersion, signer, run), keys, verifyNow)
	if err != nil {
		t.Fatalf("a run-action decision with a floor was refused: %v", err)
	}
	if got.HistoryFloorAt == nil || !got.HistoryFloorAt.Equal(at) {
		t.Fatalf("run history_floor_at = %v, want %v", got.HistoryFloorAt, at)
	}
	if err := got.CheckCanvas("org-1", "user-2", "space-room-1", false); err != nil {
		t.Fatalf("a floored run-action read was refused by the binding check: %v", err)
	}
}

// TestVerifyRunActionRefusesAFloorClaimItDoesNotName: the agent reader names the timestamp claim only. A revision claim on a run
// action is an unknown field, so the envelope is refused, as Control's own decoder refuses it.
func TestVerifyRunActionRefusesAFloorClaimItDoesNotName(t *testing.T) {
	signer := newTestSigner("run-unknown-floor")
	keys := KeySet{signer.id: signer.public()}
	token := signPayloadWith(t, RunEnvelopeVersion, signer, wellFormedRunClaims(verifyNow), "history_floor_revision", 3)
	_, err := VerifyRunAction(token, keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)
}

// TestVerifyRunActionRefusesAZeroHistoryFloor: a zero instant on a run-action read is refused, as on a human decision.
func TestVerifyRunActionRefusesAZeroHistoryFloor(t *testing.T) {
	signer := newTestSigner("run-zero-floor")
	keys := KeySet{signer.id: signer.public()}
	token := signPayloadWith(t, RunEnvelopeVersion, signer, wellFormedRunClaims(verifyNow), "history_floor_at", "0001-01-01T00:00:00Z")
	_, err := VerifyRunAction(token, keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)
}

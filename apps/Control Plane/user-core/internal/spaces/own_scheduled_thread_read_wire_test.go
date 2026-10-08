package spaces

import (
	"bytes"
	"crypto/ed25519"
	"encoding/base64"
	"testing"
	"time"
)

// D12 wire vector. Session Core re-verifies a Control read decision on its own:
// it checks the Ed25519 signature, the claim shape, and RECOMPUTES the payload
// digest from the claims. A decision this package signs and that verifier must
// therefore agree byte for byte, and the two live in different languages.
//
// This pins one real own-scheduled-thread-read decision, issued and signed here
// with fixed inputs. The same literal, key and instant are embedded in Session
// Core's `own_scheduled_thread_read_decision_issued_by_control_verifies` test
// (grpc.rs), which runs its real verifier over it. If either side changes how a
// read decision is encoded, one of the two tests fails, instead of the two
// planes silently disagreeing about what a personal Space's owner may read.
const (
	ownScheduledReadWireKeyID = "control-wire-vector-key"
	ownScheduledReadWireToken = "v2.Y29udHJvbC13aXJlLXZlY3Rvci1rZXk.eyJkZWNpc2lvbl9yZWYiOiJvd24tcmVhZC13aXJlLTEiLCJvcmdfaWQiOiJvcmctMSIsInNwYWNlX3JlZiI6InNwYWNlLXBlcnNvbmFsIiwic3ViamVjdF9pZCI6InVzZXItMSIsInNlcnZpY2VfYXVkaWVuY2UiOiJtb2RlbC1wbGFuZSIsImFjdGlvbl9pZCI6Im1vZGVsLnRocmVhZC5yZWFkIiwiYWN0aW9uX3NjaGVtYV9oYXNoIjoic2hhMjU2OnRocmVhZC1yZWFkLXYxIiwicGF5bG9hZF9kaWdlc3QiOiJzaGEyNTY6Y2E5YmM3ZjJiMDI5YzQzNWY1MGRiYjY1NWNmZDFmZGU3YTU4NjNhOTUwNDAzNjNlOGFlN2NmMzBkY2M5NjRiOCIsImlkZW1wb3RlbmN5X2tleSI6InNwYWNlLXJlYWQtd2lyZS0xIiwicmVjaXBpZW50X2F1ZGllbmNlX3JlZiI6ImF1ZGllbmNlOnNwYWNlLXBlcnNvbmFsOjIiLCJyZWNpcGllbnRfYXVkaWVuY2VfaGFzaCI6InNoYTI1Njp0ZXN0LWF1ZGllbmNlIiwicHJpdmFjeV9wb2xpY3lfcmVmIjoicHJpdmFjeTpvcmctMTo1IiwicmVzb3VyY2VfYXV0aG9yaXphdGlvbl9yZWYiOiJjb250cm9sOnNwYWNlLXBlcnNvbmFsOnRocmVhZC1yZWFkOjciLCJhdXRob3JpdHlfcmV2aXNpb24iOjcsIm1lbWJlcnNoaXBfcmV2aXNpb24iOjQsInByaXZhY3lfcmV2aXNpb24iOjUsInJlY2lwaWVudF9hdWRpZW5jZV9yZXZpc2lvbiI6MiwiZW50aXRsZW1lbnRfcmV2aXNpb24iOjMsInBlcm1pc3Npb25zIjpbInRocmVhZDpyZWFkIl0sInB1cnBvc2UiOiJhc3Npc3RhbnRfY29sbGFib3JhdGlvbiIsImxhd2Z1bF9iYXNpcyI6ImNvbnRyYWN0IiwicHJpdmFjeV9jbGFzcyI6ImludGVybmFsIiwidGhpcmRfcGFydHlfcHJvY2Vzc2luZ19hbGxvd2VkIjpmYWxzZSwicmV0ZW50aW9uX2NsYXNzIjoic3RhbmRhcmQiLCJyZXNpZGVuY3kiOiJzd2VkZW5jZW50cmFsIiwiZGVsZXRpb25fc2NvcGUiOiJzcGFjZSIsInplcm9fZGF0YV9yZXRlbnRpb24iOmZhbHNlLCJpc3N1ZWRfYXQiOiIyMDI2LTEwLTAxVDEyOjAwOjAwWiIsImV4cGlyZXNfYXQiOiIyMDI2LTEwLTAxVDEyOjAyOjAwWiIsIm5vbmNlIjoibm9uY2Utd2lyZS0xIn0.ChmOC42y-MpKrWV9T-CdsX8rjVeqepJ213aODu261XDk-AkAh13OxJXnhJ7lVYUXpdUxQjRIGC6qrxkp5ZVfCQ"
)

// The seed is public test material, never a deployment key.
func ownScheduledReadWireKey() SigningKey {
	seed := bytes.Repeat([]byte{0x2c}, ed25519.SeedSize)
	return SigningKey{ID: ownScheduledReadWireKeyID, PrivateKey: ed25519.NewKeyFromSeed(seed)}
}

func TestOwnScheduledThreadReadDecisionWireVector(t *testing.T) {
	key := ownScheduledReadWireKey()
	issuedAt := time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)
	decision, err := IssueOwnScheduledThreadReadDecision(validOwnScheduledThreadReadEvidence(), SharedThreadReadDecisionRequest{
		DecisionRef: "own-read-wire-1", IdempotencyKey: "space-read-wire-1", Nonce: "nonce-wire-1",
	}, issuedAt)
	if err != nil {
		t.Fatalf("IssueOwnScheduledThreadReadDecision: %v", err)
	}
	token, err := SignDecision(key, decision)
	if err != nil {
		t.Fatalf("SignDecision: %v", err)
	}
	if token != ownScheduledReadWireToken {
		t.Logf("public key (base64url, for the Rust side): %s",
			base64.RawURLEncoding.EncodeToString(key.PrivateKey.Public().(ed25519.PublicKey)))
		t.Fatalf("the signed own-scheduled read decision changed.\n got: %s\nwant: %s\nIf this is intended, update the literal here AND in session-core's grpc.rs test.", token, ownScheduledReadWireToken)
	}
}

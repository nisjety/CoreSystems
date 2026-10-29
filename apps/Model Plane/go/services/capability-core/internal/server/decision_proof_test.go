package server

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"
)

func TestDecisionProofBindsPolicyTupleAndVerifies(t *testing.T) {
	signer := NewDecisionProofSignerForTest()
	proof, err := signer.Sign(decisionProofClaims{
		DecisionID:        "pdec_1",
		CapabilityID:      "cap.tool.http",
		CapabilityVersion: "1.2.3",
		OrgID:             "org-a",
		RunID:             "run-a",
		AgentID:           "execution-core",
		Scope:             "global",
		Decision:          "allow",
		Reason:            "ok",
		BudgetContext:     "bounded",
	})
	if err != nil {
		t.Fatalf("Sign() error = %v", err)
	}
	parts := strings.Split(proof, ".")
	if len(parts) != 3 {
		t.Fatalf("proof has %d segments, want 3", len(parts))
	}
	decode := func(raw string) []byte {
		value, decodeErr := base64.RawURLEncoding.DecodeString(raw)
		if decodeErr != nil {
			t.Fatalf("decode proof segment: %v", decodeErr)
		}
		return value
	}
	var header decisionProofHeader
	if err := json.Unmarshal(decode(parts[0]), &header); err != nil {
		t.Fatalf("decode header: %v", err)
	}
	if header.Alg != "EdDSA" || header.Typ != decisionProofType {
		t.Fatalf("unexpected header: %+v", header)
	}
	var claims decisionProofClaims
	if err := json.Unmarshal(decode(parts[1]), &claims); err != nil {
		t.Fatalf("decode claims: %v", err)
	}
	if claims.CapabilityID != "cap.tool.http" || claims.RunID != "run-a" || claims.OrgID != "org-a" {
		t.Fatalf("policy tuple was not bound: %+v", claims)
	}
	signature := decode(parts[2])
	if len(signature) != ed25519.SignatureSize || !ed25519.Verify(signer.PublicKey(), []byte(parts[0]+"."+parts[1]), signature) {
		t.Fatal("proof signature did not verify")
	}
}

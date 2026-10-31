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
	if claims.Version != "2" {
		t.Fatalf("claims version = %q, want 2", claims.Version)
	}
	signature := decode(parts[2])
	if len(signature) != ed25519.SignatureSize || !ed25519.Verify(signer.PublicKey(), []byte(parts[0]+"."+parts[1]), signature) {
		t.Fatal("proof signature did not verify")
	}
}

func signedClaims(t *testing.T, signer *DecisionProofSigner, claims decisionProofClaims) (string, map[string]any) {
	t.Helper()
	proof, err := signer.Sign(claims)
	if err != nil {
		t.Fatalf("Sign() error = %v", err)
	}
	parts := strings.Split(proof, ".")
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		t.Fatalf("decode payload: %v", err)
	}
	var decoded map[string]any
	if err := json.Unmarshal(payload, &decoded); err != nil {
		t.Fatalf("decode claims: %v", err)
	}
	return proof, decoded
}

// Version 2 binds the tool, the person, the declared class and floor, what
// produced the decision and the pinned digest, and writes every one of them even
// when empty so the signed bytes say what was not bound.
func TestDecisionProofV2BindsToolUserClassFloorSourceAndDigest(t *testing.T) {
	signer := NewDecisionProofSignerForTest()
	digest := strings.Repeat("ab", 32)
	_, claims := signedClaims(t, signer, decisionProofClaimsFor(
		decisionProofRequest{
			capabilityID: "cap.mcp_tool.x", runID: "run-a", agentID: "execution-core", orgID: "org-a",
			scope: "global", userID: "user-a", toolName: "mcp__srv__echo",
		},
		&decisionProofResponse{
			decision: "allow", reason: "ok", budgetContext: "bounded", decisionID: "pdec_1", capabilityVersion: "1.0.0-abababababab",
			sideEffectClass: "read", approvalFloor: "none", policySource: "risk_level", toolDigest: digest,
		},
	))
	want := map[string]string{
		"v": "2", "user_id": "user-a", "tool_name": "mcp__srv__echo", "side_effect_class": "read",
		"approval_floor": "none", "policy_source": "risk_level", "tool_digest": digest,
	}
	for key, value := range want {
		if claims[key] != value {
			t.Errorf("claim %s = %v, want %q", key, claims[key], value)
		}
	}

	// A capability-level evaluation names no tool and no user; the keys are still
	// present, empty.
	_, bare := signedClaims(t, signer, decisionProofClaimsFor(
		decisionProofRequest{capabilityID: "cap.tool.http", runID: "run-a", agentID: "execution-core", orgID: "org-a", scope: "global"},
		&decisionProofResponse{decision: "allow", sideEffectClass: "read", approvalFloor: "none", policySource: "risk_level"},
	))
	for _, key := range []string{"user_id", "tool_name", "tool_digest"} {
		value, present := bare[key]
		if !present || value != "" {
			t.Errorf("a capability-level proof must carry an empty %s, got %v (present=%v)", key, value, present)
		}
	}
}

// Evidence minted for one tool does not verify once its tool name, class or
// floor is changed: the signature covers the payload bytes.
func TestDecisionProofV2SignatureCoversTheToolBinding(t *testing.T) {
	signer := NewDecisionProofSignerForTest()
	proof, _ := signedClaims(t, signer, decisionProofClaims{
		DecisionID: "pdec_1", CapabilityID: "cap.tool.http", CapabilityVersion: "1", OrgID: "org-a", RunID: "run-a",
		AgentID: "execution-core", Scope: "global", Decision: "allow", ToolName: "web_fetch",
		SideEffectClass: "read", ApprovalFloor: "none", PolicySource: "risk_level",
	})
	parts := strings.Split(proof, ".")
	signature, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		t.Fatalf("decode signature: %v", err)
	}
	verifies := func(payload []byte) bool {
		return ed25519.Verify(signer.PublicKey(), []byte(parts[0]+"."+base64.RawURLEncoding.EncodeToString(payload)), signature)
	}
	original, _ := base64.RawURLEncoding.DecodeString(parts[1])
	if !verifies(original) {
		t.Fatal("the untouched proof does not verify")
	}
	for _, swap := range [][2]string{
		{`"tool_name":"web_fetch"`, `"tool_name":"browser.act"`},
		{`"side_effect_class":"read"`, `"side_effect_class":"external_write"`},
		{`"approval_floor":"none"`, `"approval_floor":"ask"`},
		{`"policy_source":"risk_level"`, `"policy_source":"approval_floor"`},
	} {
		if !strings.Contains(string(original), swap[0]) {
			t.Fatalf("payload lacks %s: %s", swap[0], original)
		}
		if verifies([]byte(strings.Replace(string(original), swap[0], swap[1], 1))) {
			t.Errorf("proof still verifies after %s -> %s", swap[0], swap[1])
		}
	}
}

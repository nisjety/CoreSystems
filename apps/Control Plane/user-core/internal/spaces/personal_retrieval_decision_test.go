package spaces

import (
	"strings"
	"testing"
	"time"
)

func validPersonalRetrievalEvidence() PersonalThreadDecisionEvidence {
	evidence := validPersonalThreadEvidence()
	evidence.RetrievalReadEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:retrieval-read:7"
	return evidence
}

func TestIssuePersonalRetrievalDecisionIsTargetBoundAndSeparateFromThreadCreate(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	decision, err := IssuePersonalRetrievalDecision(validPersonalRetrievalEvidence(), PersonalRetrievalDecisionRequest{
		DecisionRef: "retrieval-decision-1", IdempotencyKey: "retrieval-1", Nonce: "nonce-1",
	}, now)
	if err != nil {
		t.Fatalf("IssuePersonalRetrievalDecision: %v", err)
	}
	if decision.ActionID != personalRetrievalAction || decision.ServiceAudience != personalRetrievalAudience || decision.ActionSchemaHash != personalRetrievalSchema {
		t.Fatalf("incorrect retrieval target: %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "retrieval:read" {
		t.Fatalf("retrieval permission = %#v", decision.Permissions)
	}
	if !strings.Contains(decision.ResourceAuthorizationRef, ":retrieval-read:") || strings.Contains(decision.ResourceAuthorizationRef, "thread-create") {
		t.Fatalf("retrieval resource ref widened or reused thread authority: %q", decision.ResourceAuthorizationRef)
	}
	if !strings.HasPrefix(decision.PayloadDigest, "sha256:") {
		t.Fatalf("retrieval payload digest missing: %q", decision.PayloadDigest)
	}
}

func TestIssuePersonalRetrievalDecisionDeniesThreadOnlyEntitlement(t *testing.T) {
	evidence := validPersonalRetrievalEvidence()
	evidence.RetrievalReadEntitled = false
	if _, err := IssuePersonalRetrievalDecision(evidence, PersonalRetrievalDecisionRequest{
		DecisionRef: "retrieval-decision-1", IdempotencyKey: "retrieval-1", Nonce: "nonce-1",
	}, time.Now().UTC()); err == nil || !strings.Contains(err.Error(), "retrieval entitlement") {
		t.Fatalf("thread-only policy unexpectedly issued retrieval decision: %v", err)
	}
}

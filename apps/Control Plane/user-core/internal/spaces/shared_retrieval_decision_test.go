package spaces

import (
	"strings"
	"testing"
	"time"
)

func validSharedRetrievalEvidence() PersonalThreadDecisionEvidence {
	evidence := validPersonalThreadEvidence()
	evidence.Membership.Kind = KindRoom
	evidence.Membership.Role = "viewer"
	evidence.RecipientAudienceRef = "space:room-1:recipient-audience:4"
	evidence.RecipientAudienceHash = "sha256:shared-audience"
	evidence.Membership.Revisions.RecipientAudience = 4
	evidence.ThreadCreateEntitled = false
	evidence.RetrievalReadEntitled = true
	evidence.ResourceAuthorizationRef = "control:room-1:retrieval-read:7"
	return evidence
}

func TestIssueSharedRetrievalDecisionIsTargetBoundAndSeparateFromThreadCreate(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	decision, err := IssueSharedRetrievalDecision(validSharedRetrievalEvidence(), PersonalRetrievalDecisionRequest{
		DecisionRef: "retrieval-decision-1", IdempotencyKey: "retrieval-1", Nonce: "nonce-1",
	}, now)
	if err != nil {
		t.Fatalf("IssueSharedRetrievalDecision: %v", err)
	}
	if decision.ActionID != retrievalReadAction || decision.ServiceAudience != retrievalReadAudience || decision.ActionSchemaHash != retrievalReadSchema {
		t.Fatalf("incorrect retrieval target: %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "retrieval:read" {
		t.Fatalf("retrieval permission = %#v", decision.Permissions)
	}
	if !strings.Contains(decision.ResourceAuthorizationRef, ":retrieval-read:") || strings.Contains(decision.ResourceAuthorizationRef, "thread-create") {
		t.Fatalf("shared retrieval resource ref widened or reused thread authority: %q", decision.ResourceAuthorizationRef)
	}
}

// A viewer may retrieve in a shared Space even though a viewer cannot create
// a thread there (ValidateForSharedThread requires editor/manager/owner) —
// retrieval has a lower role floor than a durable write, matching the
// personal-Space retrieval floor in validatePersonalAuthority.
func TestIssueSharedRetrievalDecisionAllowsViewerRoleUnlikeThreadCreate(t *testing.T) {
	evidence := validSharedRetrievalEvidence()
	if evidence.Membership.Role != "viewer" {
		t.Fatalf("fixture role = %q, want viewer", evidence.Membership.Role)
	}
	if _, err := IssueSharedRetrievalDecision(evidence, PersonalRetrievalDecisionRequest{
		DecisionRef: "retrieval-decision-1", IdempotencyKey: "retrieval-1", Nonce: "nonce-1",
	}, time.Now().UTC()); err != nil {
		t.Fatalf("viewer role unexpectedly denied shared retrieval: %v", err)
	}
	if err := evidence.ValidateForSharedThread(); err == nil {
		t.Fatal("fixture role should not be sufficient for shared thread creation")
	}
}

func TestIssueSharedRetrievalDecisionDeniesThreadOnlyEntitlement(t *testing.T) {
	evidence := validSharedRetrievalEvidence()
	evidence.RetrievalReadEntitled = false
	evidence.ThreadCreateEntitled = true
	if _, err := IssueSharedRetrievalDecision(evidence, PersonalRetrievalDecisionRequest{
		DecisionRef: "retrieval-decision-1", IdempotencyKey: "retrieval-1", Nonce: "nonce-1",
	}, time.Now().UTC()); err == nil || !strings.Contains(err.Error(), "incomplete") {
		t.Fatalf("thread-only entitlement unexpectedly issued a shared retrieval decision: %v", err)
	}
}

func TestIssueSharedRetrievalDecisionRequiresAResolvedAudience(t *testing.T) {
	evidence := validSharedRetrievalEvidence()
	evidence.RecipientAudienceRef = ""
	if _, err := IssueSharedRetrievalDecision(evidence, PersonalRetrievalDecisionRequest{
		DecisionRef: "retrieval-decision-1", IdempotencyKey: "retrieval-1", Nonce: "nonce-1",
	}, time.Now().UTC()); err == nil {
		t.Fatal("shared retrieval decision without a Control-resolved recipient audience unexpectedly issued")
	}
}

func TestIssueSharedRetrievalDecisionRejectsAPersonalSpace(t *testing.T) {
	evidence := validSharedRetrievalEvidence()
	evidence.Membership.Kind = KindPersonal
	if _, err := IssueSharedRetrievalDecision(evidence, PersonalRetrievalDecisionRequest{
		DecisionRef: "retrieval-decision-1", IdempotencyKey: "retrieval-1", Nonce: "nonce-1",
	}, time.Now().UTC()); err == nil || !strings.Contains(err.Error(), "non-personal Space") {
		t.Fatalf("personal Space unexpectedly issued a shared retrieval decision: %v", err)
	}
}

// The personal and shared retrieval issuers must never be interchangeable:
// personal evidence (RecipientSubjectID pinned to the sole member) issued
// through the shared path, or shared evidence issued through the personal
// path, would blur an intentionally separate authority boundary.
func TestPersonalAndSharedRetrievalIssuersRejectTheOtherKindsEvidence(t *testing.T) {
	now := time.Now().UTC()
	request := PersonalRetrievalDecisionRequest{DecisionRef: "d", IdempotencyKey: "i", Nonce: "n"}

	personalEvidence := validPersonalRetrievalEvidence()
	if _, err := IssueSharedRetrievalDecision(personalEvidence, request, now); err == nil {
		t.Fatal("shared retrieval issuer unexpectedly accepted personal Space evidence")
	}

	sharedEvidence := validSharedRetrievalEvidence()
	if _, err := IssuePersonalRetrievalDecision(sharedEvidence, request, now); err == nil {
		t.Fatal("personal retrieval issuer unexpectedly accepted a non-personal Space")
	}
}

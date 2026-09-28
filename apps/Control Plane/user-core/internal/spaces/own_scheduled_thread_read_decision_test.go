package spaces

import (
	"strings"
	"testing"
	"time"
)

// validOwnScheduledThreadReadEvidence mirrors validPersonalThreadEvidence but
// flips the entitlement the new validator actually checks (ThreadReadEntitled
// rather than ThreadCreateEntitled) and points ResourceAuthorizationRef at the
// read resource, matching what
// Repository.ResolveOwnScheduledThreadReadDecisionEvidence derives.
func validOwnScheduledThreadReadEvidence() PersonalThreadDecisionEvidence {
	evidence := validPersonalThreadEvidence()
	evidence.ThreadCreateEntitled = false
	evidence.ThreadReadEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-read:7"
	return evidence
}

func TestValidateForOwnScheduledThreadReadAcceptsPersonalOwner(t *testing.T) {
	if err := validOwnScheduledThreadReadEvidence().ValidateForOwnScheduledThreadRead(); err != nil {
		t.Fatalf("owner of their own personal Space was refused: %v", err)
	}
}

func TestValidateForOwnScheduledThreadReadRejectsRoomKind(t *testing.T) {
	evidence := validOwnScheduledThreadReadEvidence()
	evidence.Membership.Kind = KindRoom
	if err := evidence.ValidateForOwnScheduledThreadRead(); err == nil || !strings.Contains(err.Error(), "personal Space only") {
		t.Fatalf("a Room membership unexpectedly satisfied the personal-only own-scheduled read decision: %v", err)
	}
}

func TestValidateForOwnScheduledThreadReadRejectsNonOwnerRole(t *testing.T) {
	for _, role := range []string{"viewer", "editor", "manager"} {
		evidence := validOwnScheduledThreadReadEvidence()
		evidence.Membership.Role = role
		if err := evidence.ValidateForOwnScheduledThreadRead(); err == nil || !strings.Contains(err.Error(), "own owner") {
			t.Fatalf("role %q unexpectedly satisfied the owner-only own-scheduled read decision: %v", role, err)
		}
	}
}

func TestValidateForOwnScheduledThreadReadRequiresTheReadEntitlementNotCreate(t *testing.T) {
	evidence := validOwnScheduledThreadReadEvidence()
	evidence.ThreadReadEntitled = false
	evidence.ThreadCreateEntitled = true // a personal Space owner always has this; must not substitute.
	if err := evidence.ValidateForOwnScheduledThreadRead(); err == nil || !strings.Contains(err.Error(), "incomplete") {
		t.Fatalf("thread-create entitlement alone unexpectedly satisfied the own-scheduled READ decision: %v", err)
	}
}

func TestValidateForOwnScheduledThreadReadRequiresCompletePrivacyEvidence(t *testing.T) {
	evidence := validOwnScheduledThreadReadEvidence()
	evidence.Privacy.PolicyRef = ""
	if err := evidence.ValidateForOwnScheduledThreadRead(); err == nil {
		t.Fatal("incomplete privacy evidence unexpectedly satisfied the own-scheduled read decision")
	}
}

func TestIssueOwnScheduledThreadReadDecisionUsesTheSameActionAsSharedRead(t *testing.T) {
	now := time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC)
	decision, err := IssueOwnScheduledThreadReadDecision(validOwnScheduledThreadReadEvidence(), SharedThreadReadDecisionRequest{
		DecisionRef: "own-read-decision-1", IdempotencyKey: "own-read-1", Nonce: "nonce-1",
	}, now)
	if err != nil {
		t.Fatalf("IssueOwnScheduledThreadReadDecision: %v", err)
	}
	// Session Core's verifier must accept this exactly the way it accepts a
	// shared-room read decision: same action id and schema hash, so no
	// verifier change is required for this fix.
	if decision.ActionID != threadReadAction || decision.ActionSchemaHash != threadReadSchema {
		t.Fatalf("own-scheduled read decision used a different action/schema than the shared read path: %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "thread:read" {
		t.Fatalf("permissions = %#v", decision.Permissions)
	}
	if decision.ServiceAudience != ThreadReadAudienceSessionCore {
		t.Fatalf("service audience = %q, want the default Session Core audience", decision.ServiceAudience)
	}
}

func TestIssueOwnScheduledThreadReadDecisionRejectsARoomMembership(t *testing.T) {
	evidence := validOwnScheduledThreadReadEvidence()
	evidence.Membership.Kind = KindRoom
	if _, err := IssueOwnScheduledThreadReadDecision(evidence, SharedThreadReadDecisionRequest{
		DecisionRef: "own-read-decision-1", IdempotencyKey: "own-read-1", Nonce: "nonce-1",
	}, time.Now().UTC()); err == nil {
		t.Fatal("a Room membership unexpectedly obtained an own-scheduled personal-Space read decision")
	}
}

func TestIssueOwnScheduledThreadReadDecisionRejectsInvalidRequest(t *testing.T) {
	if _, err := IssueOwnScheduledThreadReadDecision(validOwnScheduledThreadReadEvidence(), SharedThreadReadDecisionRequest{
		DecisionRef: "", IdempotencyKey: "own-read-1", Nonce: "nonce-1",
	}, time.Now().UTC()); err == nil {
		t.Fatal("an empty decision_ref was unexpectedly accepted")
	}
}

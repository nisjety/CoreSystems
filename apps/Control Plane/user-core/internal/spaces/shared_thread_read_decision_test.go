package spaces

import (
	"strings"
	"testing"
	"time"
)

func validSharedThreadReadEvidence() PersonalThreadDecisionEvidence {
	evidence := validPersonalThreadEvidence()
	evidence.Membership.Kind = KindRoom
	evidence.Membership.Role = "viewer"
	evidence.RecipientAudienceRef = "space:room-1:recipient-audience:4"
	evidence.RecipientAudienceHash = "sha256:shared-audience"
	evidence.Membership.Revisions.RecipientAudience = 4
	evidence.ThreadCreateEntitled = false
	evidence.RetrievalReadEntitled = false
	evidence.ThreadReadEntitled = true
	evidence.ResourceAuthorizationRef = "control:room-1:thread-read:7"
	return evidence
}

func TestIssueSharedThreadReadDecisionIsTargetBoundAndSeparateFromCreate(t *testing.T) {
	now := time.Date(2026, 9, 6, 12, 0, 0, 0, time.UTC)
	decision, err := IssueSharedThreadReadDecision(validSharedThreadReadEvidence(), SharedThreadReadDecisionRequest{
		DecisionRef: "read-decision-1", IdempotencyKey: "read-1", Nonce: "nonce-1",
	}, now)
	if err != nil {
		t.Fatalf("IssueSharedThreadReadDecision: %v", err)
	}
	if decision.ActionID != threadReadAction || decision.ActionSchemaHash != threadReadSchema {
		t.Fatalf("incorrect read target: %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "thread:read" {
		t.Fatalf("read permission = %#v", decision.Permissions)
	}
	if decision.ActionID == personalThreadCreateAction || decision.ActionSchemaHash == personalThreadCreateSchema {
		t.Fatal("a read decision must not reuse the thread-create effect")
	}
	if !strings.Contains(decision.ResourceAuthorizationRef, ":thread-read:") || strings.Contains(decision.ResourceAuthorizationRef, "thread-create") {
		t.Fatalf("shared read resource ref widened or reused create authority: %q", decision.ResourceAuthorizationRef)
	}
	if decision.RecipientAudienceRevision != 4 {
		t.Fatalf("read decision must carry the caller's current audience revision, got %d", decision.RecipientAudienceRevision)
	}
}

// A viewer may read the room even though a viewer cannot create a thread in it.
// Reading the shared record is what a viewer role is for.
func TestIssueSharedThreadReadDecisionAllowsViewerRoleUnlikeThreadCreate(t *testing.T) {
	evidence := validSharedThreadReadEvidence()
	if evidence.Membership.Role != "viewer" {
		t.Fatalf("fixture role = %q, want viewer", evidence.Membership.Role)
	}
	if _, err := IssueSharedThreadReadDecision(evidence, SharedThreadReadDecisionRequest{
		DecisionRef: "read-decision-1", IdempotencyKey: "read-1", Nonce: "nonce-1",
	}, time.Now().UTC()); err != nil {
		t.Fatalf("viewer role unexpectedly denied a shared read: %v", err)
	}
	if err := evidence.ValidateForSharedThread(); err == nil {
		t.Fatal("fixture role should not be sufficient for shared thread creation")
	}
}

// The whole reason reads get their own bit: being allowed to speak in a room
// is not the same permission as reading everyone else's turns in it.
func TestIssueSharedThreadReadDecisionDeniesCreateOnlyEntitlement(t *testing.T) {
	evidence := validSharedThreadReadEvidence()
	evidence.ThreadReadEntitled = false
	evidence.ThreadCreateEntitled = true
	evidence.RetrievalReadEntitled = true
	if _, err := IssueSharedThreadReadDecision(evidence, SharedThreadReadDecisionRequest{
		DecisionRef: "read-decision-1", IdempotencyKey: "read-1", Nonce: "nonce-1",
	}, time.Now().UTC()); err == nil || !strings.Contains(err.Error(), "incomplete") {
		t.Fatalf("create/retrieval entitlement unexpectedly issued a shared read decision: %v", err)
	}
}

// Personal Spaces keep the owner-bound read path. Issuing here would add a
// second, weaker way to reach rows their only member can already read.
func TestIssueSharedThreadReadDecisionRefusesPersonalSpace(t *testing.T) {
	evidence := validSharedThreadReadEvidence()
	evidence.Membership.Kind = KindPersonal
	if _, err := IssueSharedThreadReadDecision(evidence, SharedThreadReadDecisionRequest{
		DecisionRef: "read-decision-1", IdempotencyKey: "read-1", Nonce: "nonce-1",
	}, time.Now().UTC()); err == nil || !strings.Contains(err.Error(), "non-personal") {
		t.Fatalf("personal Space unexpectedly issued a shared read decision: %v", err)
	}
}

// An absent audience means Control could not prove the caller is a current
// recipient. That must fail closed rather than fall back to membership alone.
func TestIssueSharedThreadReadDecisionRequiresCurrentRecipientAudience(t *testing.T) {
	evidence := validSharedThreadReadEvidence()
	evidence.RecipientAudienceRef = "  "
	if _, err := IssueSharedThreadReadDecision(evidence, SharedThreadReadDecisionRequest{
		DecisionRef: "read-decision-1", IdempotencyKey: "read-1", Nonce: "nonce-1",
	}, time.Now().UTC()); err == nil || !strings.Contains(err.Error(), "recipient audience") {
		t.Fatalf("missing recipient audience unexpectedly issued a read decision: %v", err)
	}
}

// The digest must move with the authority it was issued under, so a token
// minted before a membership change cannot be replayed after one.
func TestSharedThreadReadPayloadDigestBindsAudienceRevision(t *testing.T) {
	request := SharedThreadReadDecisionRequest{DecisionRef: "read-decision-1", IdempotencyKey: "read-1", Nonce: "nonce-1"}
	first := sharedThreadReadPayloadDigest(validSharedThreadReadEvidence(), request)
	changed := validSharedThreadReadEvidence()
	changed.Membership.Revisions.RecipientAudience = 5
	if first == sharedThreadReadPayloadDigest(changed, request) {
		t.Fatal("read digest ignored the recipient audience revision")
	}
	rotated := validSharedThreadReadEvidence()
	rotated.Membership.Revisions.Authority = 99
	if first == sharedThreadReadPayloadDigest(rotated, request) {
		t.Fatal("read digest ignored the authority revision")
	}
}

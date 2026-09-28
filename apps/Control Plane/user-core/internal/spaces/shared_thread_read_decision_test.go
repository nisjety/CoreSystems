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

// The default audience is load-bearing for compatibility, not a detail: every
// caller that existed before S4.2 omits the field, and Session Core's verifier
// compares the audience against its own constant. If this ever stopped
// defaulting to Session Core, the Work tab's conversation reads would start
// failing at a verifier three services away with nothing pointing back here.
func TestIssueSharedThreadReadDecisionDefaultsToSessionCoreAudience(t *testing.T) {
	now := time.Date(2026, 9, 13, 12, 0, 0, 0, time.UTC)
	decision, err := IssueSharedThreadReadDecision(validSharedThreadReadEvidence(), SharedThreadReadDecisionRequest{
		DecisionRef: "read-decision-1", IdempotencyKey: "read-1", Nonce: "nonce-1",
	}, now)
	if err != nil {
		t.Fatalf("IssueSharedThreadReadDecision: %v", err)
	}
	if decision.ServiceAudience != ThreadReadAudienceSessionCore {
		t.Fatalf("default audience = %q, want %q", decision.ServiceAudience, ThreadReadAudienceSessionCore)
	}
	if decision.ServiceAudience != "model-plane" {
		t.Fatalf("the Session Core audience literal changed to %q; Session Core's own verifier pins the old value and will refuse every decision", decision.ServiceAudience)
	}
}

// The sandbox-manager audience carries the SAME authority to a different
// recipient: same action, same permission, same revisions. Only the addressee
// changes, which is what makes this a second audience rather than a second
// kind of decision.
func TestIssueSharedThreadReadDecisionForSandboxManagerKeepsTheSameAuthority(t *testing.T) {
	now := time.Date(2026, 9, 13, 12, 0, 0, 0, time.UTC)
	base := SharedThreadReadDecisionRequest{DecisionRef: "read-decision-1", IdempotencyKey: "read-1", Nonce: "nonce-1"}
	forSession, err := IssueSharedThreadReadDecision(validSharedThreadReadEvidence(), base, now)
	if err != nil {
		t.Fatalf("session-core decision: %v", err)
	}
	scoped := base
	scoped.ServiceAudience = ThreadReadAudienceSandboxManager
	forSandbox, err := IssueSharedThreadReadDecision(validSharedThreadReadEvidence(), scoped, now)
	if err != nil {
		t.Fatalf("sandbox-manager decision: %v", err)
	}
	if forSandbox.ServiceAudience != "model-plane-sandbox-manager" {
		t.Fatalf("audience = %q", forSandbox.ServiceAudience)
	}
	if forSandbox.ActionID != forSession.ActionID || forSandbox.ActionSchemaHash != forSession.ActionSchemaHash {
		t.Fatal("the two audiences must describe the same effect; a different action would be a second authority to keep in sync")
	}
	if len(forSandbox.Permissions) != 1 || forSandbox.Permissions[0] != "thread:read" {
		t.Fatalf("sandbox-manager read permissions = %#v; a read decision must never widen", forSandbox.Permissions)
	}
	if forSandbox.RecipientAudienceRevision != forSession.RecipientAudienceRevision {
		t.Fatal("both audiences must carry the same audience-revision ceiling")
	}
	// The digest formula must NOT have picked up the audience: Session Core
	// recomputes it independently, so a change here silently invalidates every
	// decision that service has ever been issued.
	if forSandbox.PayloadDigest != forSession.PayloadDigest {
		t.Fatal("the payload digest changed with the audience; Session Core recomputes this formula and would reject its own decisions")
	}
}

// An unrecognized audience is refused at issuance. Signing it instead would
// produce a token no recipient accepts, surfacing as an unexplained denial in
// whichever service the caller eventually tried — the same failure mode S4.2
// step 3 closed for the `processes` claim.
func TestIssueSharedThreadReadDecisionRefusesAnUnknownAudience(t *testing.T) {
	// `model-plane-session-core` is in this list on purpose: it is the name the
	// S4.2 design used for the default recipient, and it is NOT the live
	// constant. Signing it would produce a token Session Core refuses.
	for _, audience := range []string{"model-plane-session-core", "sandbox-manager", "data-plane", "model-plane-sandbox-manager-x"} {
		request := SharedThreadReadDecisionRequest{
			DecisionRef: "read-decision-1", IdempotencyKey: "read-1", Nonce: "nonce-1",
			ServiceAudience: audience,
		}
		if _, err := IssueSharedThreadReadDecision(validSharedThreadReadEvidence(), request, time.Now().UTC()); err == nil {
			t.Fatalf("audience %q was signed; it is not a recipient any Model Plane service verifies", audience)
		}
	}
	// Whitespace-only is "unspecified", not "invalid" — the field is optional
	// and every other optional string in this package is read through
	// TrimSpace. Refusing it would make a stray space in a caller's JSON a
	// 403 rather than the default it obviously means.
	blank := SharedThreadReadDecisionRequest{
		DecisionRef: "read-decision-1", IdempotencyKey: "read-1", Nonce: "nonce-1",
		ServiceAudience: "   ",
	}
	decision, err := IssueSharedThreadReadDecision(validSharedThreadReadEvidence(), blank, time.Now().UTC())
	if err != nil {
		t.Fatalf("a blank audience must default rather than fail: %v", err)
	}
	if decision.ServiceAudience != ThreadReadAudienceSessionCore {
		t.Fatalf("blank audience resolved to %q, want the Session Core default", decision.ServiceAudience)
	}
}

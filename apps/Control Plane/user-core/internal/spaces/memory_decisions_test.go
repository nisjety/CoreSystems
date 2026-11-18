package spaces

import (
	"strings"
	"testing"
	"time"
)

// WS17-S7 + W3-FIX-5: the declassify/retract decision (pure logic). The repository join
// is covered by the live tier; this pins the request shape, the actor/audience/entitlement
// rules (including the retract asymmetry), the digest trim fix and the Control<->Session
// Core payload digest (memory_declassify_payload_digest in session-core grpc.rs).

func memoryDeclassifyEvidence(role string) PersonalThreadDecisionEvidence {
	e := validPersonalThreadEvidence()
	e.Membership.Kind = KindRoom
	e.Membership.SpaceRef = "space-room"
	e.Membership.Role = role
	e.RecipientAudienceRef = "audience:space-room:2"
	e.RecipientAudienceHash = "sha256:shared-audience"
	e.ResourceAuthorizationRef = "control:space-room:memory-declassify:7"
	e.MemoryDeclassifyEntitled = true
	return e
}

func memoryDeclassifyRequest(operation, memoryID, contentDigest string) MemoryDeclassifyDecisionRequest {
	return MemoryDeclassifyDecisionRequest{
		DecisionRef: "decision-1", Operation: operation, MemoryID: memoryID, ContentDigest: contentDigest,
		IdempotencyKey: "decision-1", Nonce: "nonce-1",
	}
}

var memoryDecisionsNow = time.Date(2026, 11, 17, 12, 0, 0, 0, time.UTC)

// The hex vectors were computed by session-core's independent Rust implementation of this
// same digest (apps/Model Plane/rust/services/session-core/src/grpc.rs,
// memory_declassify_digest_matches_the_independent_golden_vectors, over the fixed claims
// declassify_claims builds), so a drift on either side of the Control/Session Core boundary
// shows up here. Session Core verifies the digest with its own copy
// (memory_declassify_payload_digest), so this is the cross-plane contract, not just a Go
// regression pin.
func TestMemoryDeclassifyDigestGolden(t *testing.T) {
	evidence := PersonalThreadDecisionEvidence{
		Membership: CurrentMembership{
			SpaceRef: "space-room", OrgID: "org-1", SubjectID: "user-1", Kind: KindRoom, Role: "owner",
			Revisions: AuthorityRevision{Authority: 7, Membership: 4, Privacy: 5, RecipientAudience: 4, Entitlement: 3},
		},
		RecipientAudienceRef:  "audience:space-room:4",
		RecipientAudienceHash: "sha256:test-audience",
		Privacy: PrivacyPolicySnapshot{
			PolicyRef: "privacy:org-1:5", Purpose: "assistant_collaboration", LawfulBasis: "contract",
			PrivacyClass: "internal", RetentionClass: "standard", Residency: "swedencentral", DeletionScope: "space",
		},
		ResourceAuthorizationRef: "control:space-room:memory-declassify:7",
		MemoryDeclassifyEntitled: true,
	}

	share, err := IssueMemoryDeclassifyDecision(evidence, MemoryDeclassifyDecisionRequest{
		DecisionRef: "decision-1", Operation: MemoryDeclassifyOperationDeclassify, MemoryID: "memory-1",
		ContentDigest:  "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
		IdempotencyKey: "declassify-1", Nonce: "nonce-1",
	}, memoryDecisionsNow)
	if err != nil {
		t.Fatalf("declassify: %v", err)
	}
	if want := "sha256:3a23fef9d9603273cf7b1ff31f85f5e2c764565a438338ec4975c8309525fb8e"; share.PayloadDigest != want {
		t.Errorf("declassify digest = %s, want %s", share.PayloadDigest, want)
	}

	retract, err := IssueMemoryDeclassifyDecision(evidence, MemoryDeclassifyDecisionRequest{
		DecisionRef: "decision-1", Operation: MemoryDeclassifyOperationRetract, MemoryID: "room-memory-1",
		IdempotencyKey: "retract-1", Nonce: "nonce-1",
	}, memoryDecisionsNow)
	if err != nil {
		t.Fatalf("retract: %v", err)
	}
	if want := "sha256:0a54fa8304deaa9766eed6812aff77b2a07a615b8214e2ac21bafc59ea653e8f"; retract.PayloadDigest != want {
		t.Errorf("retract digest = %s, want %s", retract.PayloadDigest, want)
	}
}

func TestMemoryDecisionsRefusedForViewerNonRecipientNoEntitlementAndZDR(t *testing.T) {
	declassifyRequest := memoryDeclassifyRequest(MemoryDeclassifyOperationDeclassify, "memory-1", "sha256:"+strings.Repeat("a", 64))
	retractRequest := memoryDeclassifyRequest(MemoryDeclassifyOperationRetract, "room-memory-1", "")

	// Declassify is refused for a viewer, a non-recipient, the entitlement bit off, and
	// under Zero Data Retention.
	for name, tc := range map[string]struct {
		mutate func(*PersonalThreadDecisionEvidence)
		code   DenialCode
	}{
		"viewer":              {func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = "viewer" }, DenialRoleBelowFloor},
		"non-recipient":       {func(e *PersonalThreadDecisionEvidence) { e.RecipientAudienceRef = "" }, DenialAuthorityIncomplete},
		"entitlement off":     {func(e *PersonalThreadDecisionEvidence) { e.MemoryDeclassifyEntitled = false }, DenialEntitlementOff},
		"zero data retention": {func(e *PersonalThreadDecisionEvidence) { e.Privacy.ZeroDataRetention = true }, DenialZDRForbidsEffect},
	} {
		t.Run("declassify/"+name, func(t *testing.T) {
			evidence := memoryDeclassifyEvidence("editor")
			tc.mutate(&evidence)
			_, err := IssueMemoryDeclassifyDecision(evidence, declassifyRequest, memoryDecisionsNow)
			wantDenial(t, err, tc.code)
		})
	}
	// Editor, manager and owner may all declassify.
	for _, role := range []string{"editor", "manager", "owner"} {
		if _, err := IssueMemoryDeclassifyDecision(memoryDeclassifyEvidence(role), declassifyRequest, memoryDecisionsNow); err != nil {
			t.Errorf("declassify as %s refused: %v", role, err)
		}
	}

	// Retract needs neither the entitlement bit nor ZDR clearance: it must always be
	// possible to take back a shared copy, even after the org turns the entitlement off
	// or the Space to Zero Data Retention. This is a deliberate privacy-safety asymmetry
	// (read the ValidateForMemoryRetract comment in memory_decisions.go): a durable copy
	// must never be strandable.
	for name, mutate := range map[string]func(*PersonalThreadDecisionEvidence){
		"entitlement off":     func(e *PersonalThreadDecisionEvidence) { e.MemoryDeclassifyEntitled = false },
		"zero data retention": func(e *PersonalThreadDecisionEvidence) { e.Privacy.ZeroDataRetention = true },
		"both off": func(e *PersonalThreadDecisionEvidence) {
			e.MemoryDeclassifyEntitled = false
			e.Privacy.ZeroDataRetention = true
		},
	} {
		t.Run("retract allowed/"+name, func(t *testing.T) {
			evidence := memoryDeclassifyEvidence("editor")
			mutate(&evidence)
			decision, err := IssueMemoryDeclassifyDecision(evidence, retractRequest, memoryDecisionsNow)
			if err != nil {
				t.Fatalf("retract unexpectedly refused: %v", err)
			}
			if decision.ZeroDataRetention != evidence.Privacy.ZeroDataRetention {
				t.Errorf("decision ZeroDataRetention claim = %v, want %v", decision.ZeroDataRetention, evidence.Privacy.ZeroDataRetention)
			}
		})
	}

	// But retract still needs the same actor/audience floor declassify needs: a viewer
	// or a non-recipient cannot retract either (only the entitlement bit and ZDR are
	// skipped, not membership and audience).
	for name, tc := range map[string]struct {
		mutate func(*PersonalThreadDecisionEvidence)
		code   DenialCode
	}{
		"viewer":         {func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = "viewer" }, DenialRoleBelowFloor},
		"non-recipient":  {func(e *PersonalThreadDecisionEvidence) { e.RecipientAudienceRef = "" }, DenialAuthorityIncomplete},
		"personal Space": {func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindPersonal }, DenialKindUnsupported},
	} {
		t.Run("retract/"+name, func(t *testing.T) {
			evidence := memoryDeclassifyEvidence("editor")
			tc.mutate(&evidence)
			_, err := IssueMemoryDeclassifyDecision(evidence, retractRequest, memoryDecisionsNow)
			wantDenial(t, err, tc.code)
		})
	}
}

func TestMemoryDeclassifyRequestValidation(t *testing.T) {
	validDigest := "sha256:" + strings.Repeat("a", 64)
	for name, request := range map[string]MemoryDeclassifyDecisionRequest{
		"declassify":        memoryDeclassifyRequest(MemoryDeclassifyOperationDeclassify, "memory-1", validDigest),
		"retract":           memoryDeclassifyRequest(MemoryDeclassifyOperationRetract, "room-memory-1", ""),
		"longest memory id": memoryDeclassifyRequest(MemoryDeclassifyOperationRetract, strings.Repeat("m", MaxSpaceMemoryIDLen), ""),
	} {
		if err := request.Validate(); err != nil {
			t.Errorf("%s refused: %v", name, err)
		}
	}
	for name, request := range map[string]MemoryDeclassifyDecisionRequest{
		"unknown operation":             memoryDeclassifyRequest("purge", "memory-1", validDigest),
		"operation case":                memoryDeclassifyRequest("DECLASSIFY", "memory-1", validDigest),
		"declassify without a digest":   memoryDeclassifyRequest(MemoryDeclassifyOperationDeclassify, "memory-1", ""),
		"declassify missing sha prefix": memoryDeclassifyRequest(MemoryDeclassifyOperationDeclassify, "memory-1", strings.Repeat("a", 64)),
		"declassify short digest":       memoryDeclassifyRequest(MemoryDeclassifyOperationDeclassify, "memory-1", "sha256:"+strings.Repeat("a", 63)),
		"declassify long digest":        memoryDeclassifyRequest(MemoryDeclassifyOperationDeclassify, "memory-1", "sha256:"+strings.Repeat("a", 65)),
		"declassify uppercase hex":      memoryDeclassifyRequest(MemoryDeclassifyOperationDeclassify, "memory-1", "sha256:"+strings.Repeat("A", 64)),
		// Retract must not carry a content digest: it has nothing to re-hash.
		"retract carries a digest": memoryDeclassifyRequest(MemoryDeclassifyOperationRetract, "room-memory-1", validDigest),
		"memory id missing":        memoryDeclassifyRequest(MemoryDeclassifyOperationRetract, "", ""),
		"memory id blank":          memoryDeclassifyRequest(MemoryDeclassifyOperationRetract, "   ", ""),
		"memory id padded":         memoryDeclassifyRequest(MemoryDeclassifyOperationRetract, " room-memory-1", ""),
		"memory id too long":       memoryDeclassifyRequest(MemoryDeclassifyOperationRetract, strings.Repeat("m", MaxSpaceMemoryIDLen+1), ""),
		"no decision ref":          {Operation: MemoryDeclassifyOperationRetract, MemoryID: "m", IdempotencyKey: "k", Nonce: "n"},
		"no idempotency key":       {DecisionRef: "d", Operation: MemoryDeclassifyOperationRetract, MemoryID: "m", Nonce: "n"},
		"no nonce":                 {DecisionRef: "d", Operation: MemoryDeclassifyOperationRetract, MemoryID: "m", IdempotencyKey: "k"},
	} {
		if err := request.Validate(); err == nil {
			t.Errorf("%s accepted", name)
		}
	}
}

// W3-FIX-5: a padded decision ref or idempotency key must hash exactly as its trimmed
// twin, because Session Core recomputes over the decision's signed (already-trimmed)
// fields. If memoryDeclassifyPayloadDigest stopped trimming, this would catch it.
func TestMemoryDeclassifyPayloadDigestTrimsPaddedDecisionRefAndIdempotencyKey(t *testing.T) {
	evidence := memoryDeclassifyEvidence("owner")

	trimmed := memoryDeclassifyRequest(MemoryDeclassifyOperationDeclassify, "memory-1", "sha256:"+strings.Repeat("a", 64))
	trimmed.DecisionRef = "decision-1"
	trimmed.IdempotencyKey = "idem-1"
	want := memoryDeclassifyPayloadDigest(evidence, trimmed)

	paddedRef := trimmed
	paddedRef.DecisionRef = "  decision-1  "
	if got := memoryDeclassifyPayloadDigest(evidence, paddedRef); got != want {
		t.Errorf("padded decision_ref digest = %s, want %s (trim defect)", got, want)
	}

	paddedKey := trimmed
	paddedKey.IdempotencyKey = "\tidem-1\n"
	if got := memoryDeclassifyPayloadDigest(evidence, paddedKey); got != want {
		t.Errorf("padded idempotency_key digest = %s, want %s (trim defect)", got, want)
	}

	paddedBoth := trimmed
	paddedBoth.DecisionRef = " decision-1 "
	paddedBoth.IdempotencyKey = " idem-1 "
	if got := memoryDeclassifyPayloadDigest(evidence, paddedBoth); got != want {
		t.Errorf("padded decision_ref+idempotency_key digest = %s, want %s (trim defect)", got, want)
	}
}

// W3-FIX-5: declassify and retract are different actions/schemas over the same
// resource, so their digests must never collide even for the same memory id.
func TestMemoryDeclassifyAndRetractDigestsDiffer(t *testing.T) {
	evidence := memoryDeclassifyEvidence("owner")

	declassify, err := IssueMemoryDeclassifyDecision(evidence,
		memoryDeclassifyRequest(MemoryDeclassifyOperationDeclassify, "memory-1", "sha256:"+strings.Repeat("a", 64)),
		memoryDecisionsNow)
	if err != nil {
		t.Fatalf("declassify: %v", err)
	}
	retract, err := IssueMemoryDeclassifyDecision(evidence,
		memoryDeclassifyRequest(MemoryDeclassifyOperationRetract, "memory-1", ""),
		memoryDecisionsNow)
	if err != nil {
		t.Fatalf("retract: %v", err)
	}
	if declassify.PayloadDigest == retract.PayloadDigest {
		t.Error("declassify and retract digests collide for the same memory id")
	}
}

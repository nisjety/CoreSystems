package spaces

import (
	"strings"
	"testing"
	"time"
)

// WS17-S8 (D12) + W3-DEC-CTL1: the per-turn personal-context decision (pure logic). The
// repository join is covered by the live tier; this pins the request shape, the
// actor/audience/entitlement rules, the entitlement-key binding (the CTL1 copy-paste bug
// class: memory_personal_turn, never memory_declassify), the ZDR-first check order, and
// the Control<->Session Core payload digest (verify_personal_context_decision in
// session-core space/memory_personal_context.rs).

func memoryPersonalContextEvidence(role string) PersonalThreadDecisionEvidence {
	e := validPersonalThreadEvidence()
	e.Membership.Kind = KindRoom
	e.Membership.SpaceRef = "space-room"
	e.Membership.Role = role
	e.RecipientAudienceRef = "audience:space-room:2"
	e.RecipientAudienceHash = "sha256:shared-audience"
	e.ResourceAuthorizationRef = "control:space-room:memory-personal-context:7"
	e.MemoryPersonalTurnEntitled = true
	return e
}

func memoryPersonalContextRequest(threadID string) MemoryPersonalContextDecisionRequest {
	return MemoryPersonalContextDecisionRequest{
		DecisionRef: "decision-1", ThreadID: threadID, IdempotencyKey: "decision-1", Nonce: "nonce-1",
	}
}

var memoryPersonalContextNow = time.Date(2026, 11, 17, 12, 0, 0, 0, time.UTC)

// GOLDEN_DIGEST in session-core's memory_personal_context.rs test module was computed by
// an independent implementation of the byte layout memoryPersonalContextPayloadDigest
// documents, over exactly the claims base_claims() builds there. This evidence and
// request reproduce those same claims field for field, so a drift on either side of the
// Control/Session Core boundary shows up here.
func TestMemoryPersonalContextDigestGolden(t *testing.T) {
	evidence := PersonalThreadDecisionEvidence{
		Membership: CurrentMembership{
			SpaceRef: "space-room", OrgID: "org-1", SubjectID: "user-1", Kind: KindRoom, Role: "editor",
			Revisions: AuthorityRevision{Authority: 7, Membership: 4, Privacy: 5, RecipientAudience: 4, Entitlement: 3},
		},
		RecipientAudienceRef:  "audience:space-room:4",
		RecipientAudienceHash: "sha256:test-audience",
		Privacy: PrivacyPolicySnapshot{
			PolicyRef: "privacy:org-1:5", Purpose: "assistant_collaboration", LawfulBasis: "contract",
			PrivacyClass: "internal", RetentionClass: "standard", Residency: "swedencentral", DeletionScope: "space",
		},
		ResourceAuthorizationRef:   "control:space-room:memory-personal-context:7",
		MemoryPersonalTurnEntitled: true,
	}
	request := MemoryPersonalContextDecisionRequest{
		DecisionRef: "personal-ctx-1", ThreadID: "thread-1", IdempotencyKey: "turn-1", Nonce: "nonce-1",
	}
	now := time.Date(2026, 11, 1, 12, 0, 0, 0, time.UTC)

	decision, err := IssueMemoryPersonalContextDecision(evidence, request, now)
	if err != nil {
		t.Fatalf("IssueMemoryPersonalContextDecision: %v", err)
	}
	if want := "sha256:b77c5c7add45335a9095972d9fa6692ecdc80994187948dc6847758a8f84947e"; decision.PayloadDigest != want {
		t.Errorf("digest = %s, want the session-core golden %s", decision.PayloadDigest, want)
	}
	// The thread id is bound: a decision for another thread is another digest (mirrors
	// session-core's own assertion that the same decision for another thread does not verify).
	other := request
	other.ThreadID = "thread-2"
	otherDecision, err := IssueMemoryPersonalContextDecision(evidence, other, now)
	if err != nil {
		t.Fatalf("other thread: %v", err)
	}
	if otherDecision.PayloadDigest == decision.PayloadDigest {
		t.Error("the thread id is not bound into the payload digest")
	}
}

func TestValidateForMemoryPersonalContextRoleKindZDRAndEntitlement(t *testing.T) {
	for name, tc := range map[string]struct {
		mutate func(*PersonalThreadDecisionEvidence)
		code   DenialCode
	}{
		"viewer":              {func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = "viewer" }, DenialRoleBelowFloor},
		"personal Space":      {func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindPersonal }, DenialKindUnsupported},
		"zero data retention": {func(e *PersonalThreadDecisionEvidence) { e.Privacy.ZeroDataRetention = true }, DenialZDRForbidsEffect},
		"entitlement off":     {func(e *PersonalThreadDecisionEvidence) { e.MemoryPersonalTurnEntitled = false }, DenialEntitlementOff},
	} {
		t.Run(name, func(t *testing.T) {
			evidence := memoryPersonalContextEvidence("editor")
			tc.mutate(&evidence)
			wantDenial(t, evidence.ValidateForMemoryPersonalContext(), tc.code)
		})
	}
	// Editor, manager and owner all pass; the personal-Space recipient-audience role
	// floor does not apply here (this effect only exists in a shared Space).
	for _, role := range []string{"editor", "manager", "owner"} {
		if err := memoryPersonalContextEvidence(role).ValidateForMemoryPersonalContext(); err != nil {
			t.Errorf("%s refused: %v", role, err)
		}
	}
}

// W3-DEC-CTL1: the exact copy-paste bug class this slice guards against. The
// personal-context decision has its OWN entitlement bit (memory_personal_turn); it must
// never fall back to, or be satisfiable by, the declassify bit (memory_declassify).
func TestValidateForMemoryPersonalContextChecksItsOwnEntitlementKeyNotDeclassify(t *testing.T) {
	// Only memory_declassify_entitled is true: must still be refused, and refused by name.
	wrongBit := memoryPersonalContextEvidence("editor")
	wrongBit.MemoryPersonalTurnEntitled = false
	wrongBit.MemoryDeclassifyEntitled = true
	denial := wantDenial(t, wrongBit.ValidateForMemoryPersonalContext(), DenialEntitlementOff)
	if denial.Entitlement != "memory_personal_turn" {
		t.Errorf("entitlement key = %q, want memory_personal_turn (checked memory_declassify_entitled instead)", denial.Entitlement)
	}

	// Neither bit set: same refusal, same key.
	neitherBit := memoryPersonalContextEvidence("editor")
	neitherBit.MemoryPersonalTurnEntitled = false
	neitherBit.MemoryDeclassifyEntitled = false
	denial = wantDenial(t, neitherBit.ValidateForMemoryPersonalContext(), DenialEntitlementOff)
	if denial.Entitlement != "memory_personal_turn" {
		t.Errorf("entitlement key = %q, want memory_personal_turn", denial.Entitlement)
	}

	// memory_personal_turn_entitled true (declassify bit left off): must pass.
	rightBit := memoryPersonalContextEvidence("editor")
	rightBit.MemoryPersonalTurnEntitled = true
	rightBit.MemoryDeclassifyEntitled = false
	if err := rightBit.ValidateForMemoryPersonalContext(); err != nil {
		t.Errorf("memory_personal_turn_entitled alone was refused: %v", err)
	}

	// Same through the issuer, end to end.
	if _, err := IssueMemoryPersonalContextDecision(wrongBit, memoryPersonalContextRequest("thread-1"), memoryPersonalContextNow); err == nil {
		t.Error("the declassify bit alone unexpectedly authorized a personal-context decision")
	}
	if _, err := IssueMemoryPersonalContextDecision(rightBit, memoryPersonalContextRequest("thread-1"), memoryPersonalContextNow); err != nil {
		t.Errorf("the personal-turn bit alone was refused: %v", err)
	}
}

// W3-DEC-CTL1: ZDR is checked before the entitlement bit, so under ZDR the call is
// refused with the ZDR code even when the entitlement bit is on (order matters for the
// error code a caller sees and for ValidateEntitlementChange's "every key must be
// stated" writer not masking a ZDR posture as a missing entitlement).
func TestValidateForMemoryPersonalContextChecksZDRBeforeEntitlement(t *testing.T) {
	// Entitled, but ZDR: ZDR code, not entitlement-off.
	entitledButZDR := memoryPersonalContextEvidence("editor")
	entitledButZDR.MemoryPersonalTurnEntitled = true
	entitledButZDR.Privacy.ZeroDataRetention = true
	denial := wantDenial(t, entitledButZDR.ValidateForMemoryPersonalContext(), DenialZDRForbidsEffect)
	if denial.Effect != "memory_personal_context" {
		t.Errorf("zdr effect = %q, want memory_personal_context", denial.Effect)
	}

	// Both the entitlement bit off AND ZDR: still the ZDR code, because ZDR is checked
	// first. If entitlement were checked first this would come back space_entitlement_off.
	both := memoryPersonalContextEvidence("editor")
	both.MemoryPersonalTurnEntitled = false
	both.Privacy.ZeroDataRetention = true
	wantDenial(t, both.ValidateForMemoryPersonalContext(), DenialZDRForbidsEffect)
}

func TestMemoryPersonalContextRequestValidation(t *testing.T) {
	for name, request := range map[string]MemoryPersonalContextDecisionRequest{
		"ordinary":          memoryPersonalContextRequest("thread-1"),
		"longest thread id": memoryPersonalContextRequest(strings.Repeat("t", maxMemoryPersonalContextThreadIDLen)),
	} {
		if err := request.Validate(); err != nil {
			t.Errorf("%s refused: %v", name, err)
		}
	}
	for name, request := range map[string]MemoryPersonalContextDecisionRequest{
		"thread id padded":   memoryPersonalContextRequest(" thread-1"),
		"thread id trailing": memoryPersonalContextRequest("thread-1 "),
		"thread id blank":    memoryPersonalContextRequest("   "),
		"thread id missing":  memoryPersonalContextRequest(""),
		"thread id too long": memoryPersonalContextRequest(strings.Repeat("t", maxMemoryPersonalContextThreadIDLen+1)),
		"no decision ref":    {ThreadID: "thread-1", IdempotencyKey: "k", Nonce: "n"},
		"no idempotency key": {DecisionRef: "d", ThreadID: "thread-1", Nonce: "n"},
		"no nonce":           {DecisionRef: "d", ThreadID: "thread-1", IdempotencyKey: "k"},
	} {
		if err := request.Validate(); err == nil {
			t.Errorf("%s accepted", name)
		}
	}
}

func TestMemoryPersonalContextDecisionCarriesOnlyItsPermission(t *testing.T) {
	evidence := memoryPersonalContextEvidence("manager")
	decision, err := IssueMemoryPersonalContextDecision(evidence, memoryPersonalContextRequest("thread-1"), memoryPersonalContextNow)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != MemoryPersonalContextPermission {
		t.Errorf("permissions = %v, want only %s", decision.Permissions, MemoryPersonalContextPermission)
	}
	if decision.ServiceAudience != "model-plane" || decision.ActionID != "model.memory.personal_context" ||
		decision.ActionSchemaHash != "sha256:memory-personal-context-v1" {
		t.Errorf("audience/action/schema = %q %q %q", decision.ServiceAudience, decision.ActionID, decision.ActionSchemaHash)
	}
	if err := decision.Validate(); err != nil {
		t.Fatalf("issued decision does not validate: %v", err)
	}
	if _, err := IssueMemoryPersonalContextDecision(evidence, memoryPersonalContextRequest("thread-1"), time.Time{}); err == nil {
		t.Error("a zero issuance time was accepted")
	}
}

package spaces

import (
	"strings"
	"testing"
	"time"
)

func validPersonalThreadEvidence() PersonalThreadDecisionEvidence {
	return PersonalThreadDecisionEvidence{
		Membership: CurrentMembership{
			SpaceRef: "space-personal", OrgID: "org-1", SubjectID: "user-1", Kind: KindPersonal, Role: "owner",
			Revisions: AuthorityRevision{Authority: 7, Membership: 4, Privacy: 5, RecipientAudience: 2, Entitlement: 3},
		},
		RecipientAudienceRef:     "audience:space-personal:2",
		RecipientAudienceHash:    "sha256:test-audience",
		RecipientSubjectID:       "user-1",
		ThreadCreateEntitled:     true,
		ResourceAuthorizationRef: "control:space-personal:thread-create:7",
		Privacy: PrivacyPolicySnapshot{
			PolicyRef: "privacy:org-1:5", Purpose: "assistant_collaboration", LawfulBasis: "contract",
			PrivacyClass: "internal", RetentionClass: "standard", Residency: "swedencentral", DeletionScope: "space",
		},
	}
}

func validPersonalThreadRequest() PersonalThreadDecisionRequest {
	return PersonalThreadDecisionRequest{
		DecisionRef: "decision-1", SessionKey: "session-1",
		IdempotencyKey: "thread-create:1", Nonce: "nonce-1",
	}
}

func TestPersonalThreadDecisionDerivesPayloadDigestFromTheExactPersistedEffect(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	first, err := IssuePersonalThreadCreateDecision(evidence, validPersonalThreadRequest(), now)
	if err != nil {
		t.Fatalf("IssuePersonalThreadCreateDecision: %v", err)
	}
	const canonicalDigest = "sha256:3486ffcc43f1c7e83003a6236d533faacf9c0249933180d0c1157a4992db7245"
	if first.PayloadDigest != canonicalDigest {
		t.Fatalf("canonical payload digest = %q, want %q", first.PayloadDigest, canonicalDigest)
	}
	changed := validPersonalThreadRequest()
	changed.SessionKey = "another-session"
	second, err := IssuePersonalThreadCreateDecision(evidence, changed, now)
	if err != nil {
		t.Fatalf("IssuePersonalThreadCreateDecision changed session: %v", err)
	}
	if first.PayloadDigest == second.PayloadDigest || !strings.HasPrefix(first.PayloadDigest, "sha256:") {
		t.Fatalf("effect digest was not canonically derived: first=%q second=%q", first.PayloadDigest, second.PayloadDigest)
	}
}

func TestRecipientAudienceHashIsOrderIndependentAndRejectsAmbiguousSets(t *testing.T) {
	first, err := RecipientAudienceHash("user-2", "user-1")
	if err != nil {
		t.Fatalf("RecipientAudienceHash: %v", err)
	}
	second, err := RecipientAudienceHash("user-1", "user-2")
	if err != nil || first != second || !strings.HasPrefix(first, "sha256:") {
		t.Fatalf("audience commitment must be canonical: first=%q second=%q err=%v", first, second, err)
	}
	if first != "sha256:cb3fb473b339cd4581363ee94743b61c591b83c7c14af8f7722b6d04ed0dd4da" {
		t.Fatalf("recipient audience hash changed: %q", first)
	}
	if _, err := RecipientAudienceHash("user-1", "user-1"); err == nil {
		t.Fatal("duplicate recipient unexpectedly accepted")
	}
}

func TestIssuePersonalThreadCreateDecisionBindsAllAuthorityInputs(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	decision, err := IssuePersonalThreadCreateDecision(validPersonalThreadEvidence(), validPersonalThreadRequest(), now)
	if err != nil {
		t.Fatalf("IssuePersonalThreadCreateDecision: %v", err)
	}
	if decision.ActionID != personalThreadCreateAction || decision.ServiceAudience != personalThreadCreateAudience || decision.ExpiresAt != now.Add(personalDecisionLifetime) {
		t.Fatalf("incorrect issuance binding: %+v", decision)
	}
	if decision.RecipientAudienceRef != "audience:space-personal:2" || decision.ResourceAuthorizationRef != "control:space-personal:thread-create:7" || decision.PrivacyPolicyRef != "privacy:org-1:5" {
		t.Fatalf("authority references changed: %+v", decision)
	}
	if err := decision.Validate(); err != nil {
		t.Fatalf("issued decision did not validate: %v", err)
	}
}

func TestIssuePersonalThreadCreateDecisionFailsClosedOnMissingOrBroaderAuthority(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	for name, mutate := range map[string]func(*PersonalThreadDecisionEvidence){
		"viewer cannot create":           func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = "viewer" },
		"recipient differs":              func(e *PersonalThreadDecisionEvidence) { e.RecipientSubjectID = "user-2" },
		"no entitlement":                 func(e *PersonalThreadDecisionEvidence) { e.ThreadCreateEntitled = false },
		"missing resource authorization": func(e *PersonalThreadDecisionEvidence) { e.ResourceAuthorizationRef = "" },
		"missing privacy":                func(e *PersonalThreadDecisionEvidence) { e.Privacy.Purpose = "" },
	} {
		t.Run(name, func(t *testing.T) {
			evidence := validPersonalThreadEvidence()
			mutate(&evidence)
			if _, err := IssuePersonalThreadCreateDecision(evidence, validPersonalThreadRequest(), now); err == nil {
				t.Fatal("incomplete authority unexpectedly issued a decision")
			}
		})
	}

	evidence := validPersonalThreadEvidence()
	evidence.Membership.Kind = KindRoom
	if _, err := IssuePersonalThreadCreateDecision(evidence, validPersonalThreadRequest(), now); err == nil || !strings.Contains(err.Error(), "personal Space") {
		t.Fatalf("non-personal Space issuance error = %v", err)
	}
}

func TestIssueSharedThreadDecisionRequiresAResolvedAudienceAndPreservesItsRevision(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.Membership.Kind = KindRoom
	evidence.RecipientAudienceRef = "space:room-1:recipient-audience:4"
	evidence.RecipientAudienceHash = "sha256:shared-audience"
	evidence.Membership.Revisions.RecipientAudience = 4
	decision, err := IssueSharedThreadCreateDecision(evidence, validPersonalThreadRequest(), now)
	if err != nil {
		t.Fatalf("IssueSharedThreadCreateDecision: %v", err)
	}
	if decision.RecipientAudienceRevision != 4 || decision.RecipientAudienceHash != evidence.RecipientAudienceHash {
		t.Fatalf("shared recipient audience drifted: %+v", decision)
	}
	evidence.RecipientAudienceRef = ""
	if _, err := IssueSharedThreadCreateDecision(evidence, validPersonalThreadRequest(), now); err == nil {
		t.Fatal("shared decision without a Control-resolved recipient audience unexpectedly issued")
	}
}

func TestThreadAppendDecisionIsContentBoundAndCannotReuseCreationContract(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	request := ThreadAppendDecisionRequest{
		DecisionRef: "append-decision-1", ThreadID: "thread-1",
		ContentDigest:  "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
		IdempotencyKey: "append-1", Nonce: "append-nonce",
	}
	decision, err := IssueThreadAppendDecision(evidence, request, now)
	if err != nil {
		t.Fatalf("IssueThreadAppendDecision: %v", err)
	}
	if decision.ActionID != threadAppendAction || decision.ActionSchemaHash != threadAppendSchema || decision.Permissions[0] != "thread:append" {
		t.Fatalf("append decision reused create contract: %+v", decision)
	}
	changed := request
	changed.ContentDigest = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
	second, err := IssueThreadAppendDecision(evidence, changed, now)
	if err != nil || second.PayloadDigest == decision.PayloadDigest {
		t.Fatalf("append payload was not content-bound: first=%q second=%q err=%v", decision.PayloadDigest, second.PayloadDigest, err)
	}
}

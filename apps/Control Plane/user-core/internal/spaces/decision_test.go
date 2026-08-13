package spaces

import (
	"crypto/ed25519"
	"encoding/base64"
	"testing"
	"time"
)

func TestLoadSigningKeyFromEnvRejectsMissingAndAcceptsSeed(t *testing.T) {
	if _, err := LoadSigningKeyFromEnv(func(string) string { return "" }); err == nil {
		t.Fatal("missing signing material was accepted")
	}
	seed := make([]byte, ed25519.SeedSize)
	for index := range seed {
		seed[index] = byte(index + 1)
	}
	key, err := LoadSigningKeyFromEnv(func(name string) string {
		switch name {
		case "CONTROL_SPACE_DECISION_KEY_ID":
			return "control-test"
		case "CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64":
			return base64.RawURLEncoding.EncodeToString(seed)
		default:
			return ""
		}
	})
	if err != nil || key.ID != "control-test" || len(key.PrivateKey) != ed25519.PrivateKeySize {
		t.Fatalf("valid seed was not loaded: key=%+v err=%v", key, err)
	}
}

func validDecision(now time.Time) Decision {
	return Decision{
		DecisionRef:               "decision-1",
		OrgID:                     "org-1",
		SpaceRef:                  "space-1",
		SubjectID:                 "user-1",
		ServiceAudience:           "model-plane",
		ActionID:                  "model.thread.create",
		ActionSchemaHash:          "sha256:thread-create-v1",
		PayloadDigest:             "sha256:payload-1",
		IdempotencyKey:            "thread-create:1",
		RecipientAudienceRef:      "conversation-audience-1",
		RecipientAudienceHash:     "sha256:audience-1",
		PrivacyPolicyRef:          "privacy-v3",
		ResourceAuthorizationRef:  "resource-auth-1",
		AuthorityRevision:         7,
		MembershipRevision:        4,
		PrivacyRevision:           5,
		RecipientAudienceRevision: 2,
		EntitlementRevision:       3,
		Permissions:               []string{"thread:create"},
		Purpose:                   "assistant_collaboration",
		LawfulBasis:               "contract",
		PrivacyClass:              "internal",
		ThirdPartyAllowed:         false,
		RetentionClass:            "standard",
		Residency:                 "swedencentral",
		DeletionScope:             "space",
		ZeroDataRetention:         false,
		IssuedAt:                  now.Add(-time.Minute),
		ExpiresAt:                 now.Add(time.Minute),
		Nonce:                     "nonce-1",
	}
}

func TestSignedDecisionBindsServiceAudienceAndExpiry(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	signer, verifier := testDecisionKeys(t)
	token, err := SignDecision(signer, validDecision(now))
	if err != nil {
		t.Fatalf("SignDecision: %v", err)
	}
	decision, err := VerifyDecision(verifier, token, expectedDecision(now))
	if err != nil {
		t.Fatalf("VerifyDecision: %v", err)
	}
	if decision.AuthorityRevision != 7 || decision.RecipientAudienceRef != "conversation-audience-1" || decision.ResourceAuthorizationRef != "resource-auth-1" {
		t.Fatalf("decision claims changed: %+v", decision)
	}
	wrongAudience := expectedDecision(now)
	wrongAudience.ServiceAudience = "data-plane"
	if _, err := VerifyDecision(verifier, token, wrongAudience); err == nil {
		t.Fatal("wrong target service audience was accepted")
	}
	wrongSpace := expectedDecision(now)
	wrongSpace.SpaceRef = "space-2"
	if _, err := VerifyDecision(verifier, token, wrongSpace); err == nil {
		t.Fatal("wrong Space reference was accepted")
	}
	expired := expectedDecision(now.Add(2 * time.Minute))
	if _, err := VerifyDecision(verifier, token, expired); err == nil {
		t.Fatal("expired decision was accepted")
	}
}

func TestSignedDecisionRejectsTamperAndIncompletePrivacyClaims(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	signer, verifier := testDecisionKeys(t)
	decision := validDecision(now)
	decision.Purpose = ""
	if _, err := SignDecision(signer, decision); err == nil {
		t.Fatal("incomplete privacy policy decision was signed")
	}

	token, err := SignDecision(signer, validDecision(now))
	if err != nil {
		t.Fatalf("SignDecision: %v", err)
	}
	wrongPublicKey, _, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	if _, err := VerifyDecision(VerificationKey{ID: verifier.ID, PublicKey: wrongPublicKey}, token, expectedDecision(now)); err == nil {
		t.Fatal("tampered signature was accepted")
	}
}

func TestSignedDecisionRejectsMissingResourceAuthorization(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	signer, _ := testDecisionKeys(t)
	decision := validDecision(now)
	decision.ResourceAuthorizationRef = ""
	if _, err := SignDecision(signer, decision); err == nil {
		t.Fatal("decision without owner-resource authorization was signed")
	}
}

func TestSignedDecisionRejectsUnknownKeyID(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	signer, verifier := testDecisionKeys(t)
	token, err := SignDecision(signer, validDecision(now))
	if err != nil {
		t.Fatalf("SignDecision: %v", err)
	}
	verifier.ID = "rotated-key"
	if _, err := VerifyDecision(verifier, token, expectedDecision(now)); err == nil {
		t.Fatal("decision signed by another key ID was accepted")
	}
}

func TestSignedDecisionRejectsAnotherEffectUsingTheSameAuthority(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	signer, verifier := testDecisionKeys(t)
	token, err := SignDecision(signer, validDecision(now))
	if err != nil {
		t.Fatalf("SignDecision: %v", err)
	}
	expected := expectedDecision(now)
	expected.PayloadDigest = "sha256:another-payload"
	if _, err := VerifyDecision(verifier, token, expected); err == nil {
		t.Fatal("decision was accepted for another payload")
	}
	expected = expectedDecision(now)
	expected.IdempotencyKey = "thread-create:another"
	if _, err := VerifyDecision(verifier, token, expected); err == nil {
		t.Fatal("decision was accepted for another idempotency key")
	}
}

func expectedDecision(now time.Time) DecisionExpectation {
	return DecisionExpectation{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ServiceAudience: "model-plane",
		ActionID: "model.thread.create", ActionSchemaHash: "sha256:thread-create-v1",
		PayloadDigest: "sha256:payload-1", IdempotencyKey: "thread-create:1", Now: now,
	}
}

func testDecisionKeys(t *testing.T) (SigningKey, VerificationKey) {
	t.Helper()
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	return SigningKey{ID: "control-2026-08", PrivateKey: privateKey}, VerificationKey{ID: "control-2026-08", PublicKey: publicKey}
}

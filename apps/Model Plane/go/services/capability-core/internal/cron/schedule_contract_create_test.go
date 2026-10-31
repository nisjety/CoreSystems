package cron

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// signedContractCreateToken signs a create decision with the schema that matches
// the intent, so the tests below change one binding at a time.
func signedContractCreateToken(t *testing.T, private ed25519.PrivateKey, keyID string, intent CreateIntent, schema string) string {
	t.Helper()
	now := time.Now().UTC()
	decision := controlDecision{
		DecisionRef: "decision-1", OrgID: intent.OrgID, SpaceRef: intent.SpaceRef, SubjectID: intent.SubjectID,
		ServiceAudience: controlCreateAudience, ActionID: controlCreateAction, ActionSchemaHash: schema,
		IdempotencyKey: intent.IdempotencyKey, RecipientAudienceRef: "audience-1", RecipientAudienceHash: "sha256:audience",
		PrivacyPolicyRef: "privacy-1", ResourceAuthorizationRef: "resource-1", AuthorityRevision: 7,
		MembershipRevision: 4, PrivacyRevision: 5, RecipientAudienceRevision: 2, EntitlementRevision: 3,
		Permissions: []string{"cron:create"}, IssuedAt: now.Add(-time.Second), ExpiresAt: now.Add(time.Minute),
	}
	decision.PayloadDigest = expectedCreatePayloadDigest(decision, intent)
	payload, err := json.Marshal(decision)
	if err != nil {
		t.Fatal(err)
	}
	signed := controlDecisionVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(keyID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(private, []byte(signed)))
}

func contractCreateIntent() CreateIntent {
	intent := validControlCreateIntent()
	intent.ContractDigest = "sha256:" + strings.Repeat("c", 64)
	intent.ContractExpiresAt = "2026-11-23T10:00:00Z"
	return intent
}

func TestScheduleCreateBindsContractDigestAndExpiry(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := NewControlDecisionVerifier("key-1", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	intent := contractCreateIntent()
	token := signedContractCreateToken(t, private, "key-1", intent, controlCreateContractSchema)
	if _, err := verifier.VerifyScheduleCreate(token, intent, time.Now().UTC()); err != nil {
		t.Fatalf("a contract-bound create was refused: %v", err)
	}

	tampered := intent
	tampered.ContractDigest = "sha256:" + strings.Repeat("d", 64)
	if _, err := verifier.VerifyScheduleCreate(token, tampered, time.Now().UTC()); err == nil {
		t.Fatal("a tampered contract digest was accepted")
	}
	shifted := intent
	shifted.ContractExpiresAt = "2026-12-23T10:00:00Z"
	if _, err := verifier.VerifyScheduleCreate(token, shifted, time.Now().UTC()); err == nil {
		t.Fatal("a shifted contract expiry was accepted")
	}
}

func TestScheduleCreateRejectsContractNotBoundByControl(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := NewControlDecisionVerifier("key-1", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	// Control signed a contract-free v1 decision; a template carrying a contract
	// must not ride on it.
	contractFree := validControlCreateIntent()
	v1Token := signedContractCreateToken(t, private, "key-1", contractFree, controlCreateSchema)
	if _, err := verifier.VerifyScheduleCreate(v1Token, contractCreateIntent(), time.Now().UTC()); err == nil {
		t.Fatal("a contract-free decision authorised a contract-bearing schedule")
	}
	// The reverse: a contract-bound v2 decision must not authorise a legacy create.
	v2Token := signedContractCreateToken(t, private, "key-1", contractCreateIntent(), controlCreateContractSchema)
	if _, err := verifier.VerifyScheduleCreate(v2Token, contractFree, time.Now().UTC()); err == nil {
		t.Fatal("a contract-bound decision authorised a contract-free schedule")
	}
}

func TestCreateIntentRequiresContractDigestAndExpiryTogether(t *testing.T) {
	intent := validControlCreateIntent()
	intent.ContractExpiresAt = "2026-11-23T10:00:00Z"
	if err := validateCreateIntent(intent); err == nil {
		t.Fatal("an expiry without a digest was accepted")
	}
	intent.ContractDigest = "sha256:short"
	if err := validateCreateIntent(intent); err == nil {
		t.Fatal("a malformed contract digest was accepted")
	}
}

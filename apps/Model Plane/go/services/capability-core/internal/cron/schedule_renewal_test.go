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

func signDecisionForTest(t *testing.T, private ed25519.PrivateKey, keyID string, decision controlDecision) string {
	t.Helper()
	payload, err := json.Marshal(decision)
	if err != nil {
		t.Fatal(err)
	}
	signed := controlDecisionVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(keyID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(private, []byte(signed)))
}

func renewalIntent() CreateIntent {
	intent := contractCreateIntent()
	intent.ScheduleID = "cron-1"
	intent.RenewalOf = "cron-1"
	return intent
}

func newRenewalVerifier(t *testing.T) (*ControlDecisionVerifier, ed25519.PrivateKey) {
	t.Helper()
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := NewControlDecisionVerifier("key-1", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	return verifier, private
}

// A renewal decision verifies only for the renewal intent it was signed for. Neither a create
// decision (v1 or v2) nor a renewal decision can stand in for the other.
func TestScheduleRenewalIsNotInterchangeableWithCreate(t *testing.T) {
	verifier, private := newRenewalVerifier(t)
	now := time.Now().UTC()

	renewal := renewalIntent()
	renewalToken := signedContractCreateToken(t, private, "key-1", renewal, controlRenewalSchema)
	if _, err := verifier.VerifyScheduleCreate(renewalToken, renewal, now); err != nil {
		t.Fatalf("a renewal decision was refused for its own intent: %v", err)
	}

	// The same schedule id and contract as a create must refuse the renewal decision.
	asCreate := renewal
	asCreate.RenewalOf = ""
	if _, err := verifier.VerifyScheduleCreate(renewalToken, asCreate, now); err == nil {
		t.Fatal("a renewal decision authorised a create")
	}

	// A create decision (v2, contract bound) for the same schedule must not renew it.
	createToken := signedContractCreateToken(t, private, "key-1", asCreate, controlCreateContractSchema)
	if _, err := verifier.VerifyScheduleCreate(createToken, renewal, now); err == nil {
		t.Fatal("a create decision was replayed as a renewal")
	}
	// Even a create-schema decision whose payload was computed with the renewal fields is refused.
	forged := signedContractCreateToken(t, private, "key-1", renewal, controlCreateContractSchema)
	if _, err := verifier.VerifyScheduleCreate(forged, renewal, now); err == nil {
		t.Fatal("a decision under the create schema was accepted as a renewal")
	}
}

func TestScheduleRenewalBindsTheRenewedScheduleAndContract(t *testing.T) {
	verifier, private := newRenewalVerifier(t)
	now := time.Now().UTC()
	renewal := renewalIntent()
	token := signedContractCreateToken(t, private, "key-1", renewal, controlRenewalSchema)

	other := renewal
	other.ScheduleID = "cron-2"
	other.RenewalOf = "cron-2"
	if _, err := verifier.VerifyScheduleCreate(token, other, now); err == nil {
		t.Fatal("a renewal decision for one schedule renewed another")
	}
	wrongContract := renewal
	wrongContract.ContractDigest = "sha256:" + strings.Repeat("d", 64)
	_, err := verifier.VerifyScheduleCreate(token, wrongContract, now)
	if err == nil || err != ErrDecisionPayloadMismatch {
		t.Fatalf("a different contract digest must fail as a payload mismatch, got %v", err)
	}
}

func TestRenewalIntentMustNameItsOwnScheduleAndCarryAContract(t *testing.T) {
	intent := renewalIntent()
	intent.RenewalOf = "cron-2"
	if err := validateCreateIntent(intent); err == nil {
		t.Fatal("a renewal of another schedule than the signed one was accepted")
	}
	intent = validControlCreateIntent()
	intent.RenewalOf = intent.ScheduleID
	if err := validateCreateIntent(intent); err == nil {
		t.Fatal("a renewal without a contract was accepted")
	}
}

func TestRenewalRefusesZeroDataRetention(t *testing.T) {
	verifier, private := newRenewalVerifier(t)
	renewal := renewalIntent()
	now := time.Now().UTC()
	decision := controlDecision{
		DecisionRef: "decision-1", OrgID: renewal.OrgID, SpaceRef: renewal.SpaceRef, SubjectID: renewal.SubjectID,
		ServiceAudience: controlCreateAudience, ActionID: controlCreateAction, ActionSchemaHash: controlRenewalSchema,
		IdempotencyKey: renewal.IdempotencyKey, RecipientAudienceRef: "audience-1", RecipientAudienceHash: "sha256:audience",
		PrivacyPolicyRef: "privacy-1", ResourceAuthorizationRef: "resource-1", AuthorityRevision: 7,
		MembershipRevision: 4, PrivacyRevision: 5, RecipientAudienceRevision: 2, EntitlementRevision: 3,
		Permissions: []string{"cron:create"}, ZeroDataRetention: true,
		IssuedAt: now.Add(-time.Second), ExpiresAt: now.Add(time.Minute),
	}
	decision.PayloadDigest = expectedCreatePayloadDigest(decision, renewal)
	token := signDecisionForTest(t, private, "key-1", decision)
	if _, err := verifier.VerifyScheduleCreate(token, renewal, now); err == nil {
		t.Fatal("a zero data retention decision renewed a schedule")
	}
}

// The renewal payload digest is pinned here and in user-core
// (internal/spaces/schedule_renewal_decision_test.go). The two planes hash the same fields in the
// same order, so change this value in both places or in neither.
func TestRenewalPayloadDigestMatchesControlVector(t *testing.T) {
	decision := controlDecision{
		OrgID: "org-1", SpaceRef: "space-personal", SubjectID: "user-1",
		RecipientAudienceRef: "audience:space-personal:2", RecipientAudienceHash: "sha256:test-audience",
		PrivacyPolicyRef: "privacy:org-1:5", ResourceAuthorizationRef: "control:space-personal:thread-create:7",
		AuthorityRevision: 7, MembershipRevision: 4, PrivacyRevision: 5, RecipientAudienceRevision: 2, EntitlementRevision: 3,
	}
	intent := CreateIntent{
		OrgID: "org-1", SpaceRef: "space-personal", SubjectID: "user-1", ScheduleID: "sched-1",
		TemplateDigest: "sha256:" + strings.Repeat("b", 64), IdempotencyKey: "space-cron-renew:sched-1:1",
		ContractDigest: "sha256:" + strings.Repeat("c", 64), ContractExpiresAt: "2026-11-23T10:00:00Z", RenewalOf: "sched-1",
	}
	got := expectedCreatePayloadDigest(decision, intent)
	const want = "sha256:3db5664dde4b33a12c58d5489d7ada4be80171736154ec26acd6b60efda7567f"
	if got != want {
		t.Fatalf("renewal payload digest = %s, want %s", got, want)
	}
}

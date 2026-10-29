package activities

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"testing"
	"time"
)

func validScheduledStepExecutionIntent() ScheduledStepExecutionIntent {
	return ScheduledStepExecutionIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", RunID: "run-1", ThreadID: "thread-1",
		ScheduleID: "schedule-1", FireKey: "fire-1",
		TemplateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		StepID:         "step-1", StepIndex: 0,
		PolicyDigest:   "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		IdempotencyKey: "run-1:step-1",
	}
}

func signedScheduledStepToken(t *testing.T, keyID string, private ed25519.PrivateKey, intent ScheduledStepExecutionIntent, now time.Time) string {
	t.Helper()
	decision := scheduledStepDecision{
		DecisionRef: "decision-1", OrgID: intent.OrgID, SpaceRef: intent.SpaceRef, SubjectID: intent.SubjectID,
		ServiceAudience: controlScheduledStepAudience, ActionID: controlScheduledStepAction,
		ActionSchemaHash: controlScheduledStepSchema, IdempotencyKey: intent.IdempotencyKey,
		RecipientAudienceRef: "aud-1", RecipientAudienceHash: "sha256:aud", PrivacyPolicyRef: "privacy-1",
		ResourceAuthorizationRef: "resource-1", AuthorityRevision: 1, MembershipRevision: 1,
		PrivacyRevision: 1, RecipientAudienceRevision: 1, EntitlementRevision: 1,
		Permissions: []string{"schedule:step"}, Nonce: "nonce-1", IssuedAt: now.Add(-time.Second), ExpiresAt: now.Add(time.Minute),
	}
	decision.PayloadDigest = scheduledStepPayloadDigest(decision, intent)
	payload, err := json.Marshal(decision)
	if err != nil {
		t.Fatal(err)
	}
	key := base64.RawURLEncoding.EncodeToString([]byte(keyID))
	body := base64.RawURLEncoding.EncodeToString(payload)
	signingInput := controlScheduledStepDecisionVersion + "." + key + "." + body
	signature := base64.RawURLEncoding.EncodeToString(ed25519.Sign(private, []byte(signingInput)))
	return signingInput + "." + signature
}

func TestControlScheduledStepAuthorizerBindsExactStep(t *testing.T) {
	public, private, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatal(err)
	}
	intent := validScheduledStepExecutionIntent()
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	authorizer := &ControlScheduledStepAuthorizer{keyID: "key-1", public: public}
	token := signedScheduledStepToken(t, "key-1", private, intent, now)
	if err := authorizer.verify(token, intent, now); err != nil {
		t.Fatalf("verify: %v", err)
	}
	changed := intent
	changed.StepIndex = 1
	if err := authorizer.verify(token, changed, now); err == nil {
		t.Fatal("changed step index must not reuse a signed decision")
	}
}

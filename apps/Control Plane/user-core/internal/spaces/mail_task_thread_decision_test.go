package spaces

import (
	"strings"
	"testing"
	"time"
)

func validMailTaskEffectPolicy(orgID string) EffectPolicy {
	return EffectPolicy{
		OrgID:                orgID,
		PrivacyPolicyRef:     "privacy:" + orgID + ":1",
		Purpose:              "mail-task automation",
		LawfulBasis:          "contract",
		PrivacyClass:         "internal",
		RetentionClass:       "standard",
		Residency:            "eu",
		DeletionScope:        "org",
		ZeroDataRetention:    false,
		ThreadCreateEntitled: true,
	}
}

func TestIssueMailTaskThreadDecisionRoundTripsThroughItsOwnVerifier(t *testing.T) {
	signer, verifier := testDecisionKeys(t)
	now := time.Date(2026, time.September, 28, 12, 0, 0, 0, time.UTC)
	policy := validMailTaskEffectPolicy("org-1")

	token, err := IssueMailTaskThreadDecision(policy, signer, "org-1", "user-1", "alias-1", "provider-msg-1", now)
	if err != nil {
		t.Fatalf("IssueMailTaskThreadDecision: %v", err)
	}
	if !strings.HasPrefix(token, "mail-task-v1.") {
		t.Fatalf("expected mail-task-v1 envelope, got %q", token)
	}

	decision, err := VerifyMailTaskThreadDecision(verifier, token, now.Add(time.Second))
	if err != nil {
		t.Fatalf("VerifyMailTaskThreadDecision: %v", err)
	}
	if decision.OrgID != "org-1" || decision.UserID != "user-1" || decision.AliasID != "alias-1" ||
		decision.ProviderMessageID != "provider-msg-1" || decision.Origin != "mail_task" ||
		decision.ServiceAudience != mailTaskThreadPrepareAudience || decision.ActionID != mailTaskThreadPrepareAction {
		t.Fatalf("unexpected decision claims: %#v", decision)
	}
	if decision.SessionKey != MailTaskSessionKey("org-1", "provider-msg-1") {
		t.Fatalf("session key does not match the deterministic formula: %#v", decision)
	}
}

// mail_task_thread_decision_round_trips_through_the_mail-task-v1_verifier_and_is_rejected_by_the_space-decision_verifier_and_vice_versa
func TestMailTaskThreadDecisionIsRejectedByTheSpaceDecisionVerifierAndViceVersa(t *testing.T) {
	signer, verifier := testDecisionKeys(t)
	now := time.Date(2026, time.September, 28, 12, 0, 0, 0, time.UTC)

	mailTaskToken, err := IssueMailTaskThreadDecision(validMailTaskEffectPolicy("org-1"), signer, "org-1", "user-1", "alias-1", "provider-msg-1", now)
	if err != nil {
		t.Fatalf("IssueMailTaskThreadDecision: %v", err)
	}
	if _, err := VerifyDecision(verifier, mailTaskToken, expectedDecision(now)); err == nil {
		t.Fatalf("expected the Space-decision verifier to reject a mail-task-v1 envelope")
	}

	spaceToken, err := SignDecision(signer, validDecision(now))
	if err != nil {
		t.Fatalf("SignDecision: %v", err)
	}
	if _, err := VerifyMailTaskThreadDecision(verifier, spaceToken, now); err == nil {
		t.Fatalf("expected the mail-task-v1 verifier to reject a Space \"v2\" envelope")
	}
}

func TestIssueMailTaskThreadDecisionRefusesForAZDROrg(t *testing.T) {
	signer, _ := testDecisionKeys(t)
	now := time.Date(2026, time.September, 28, 12, 0, 0, 0, time.UTC)
	policy := validMailTaskEffectPolicy("org-1")
	policy.ZeroDataRetention = true

	if _, err := IssueMailTaskThreadDecision(policy, signer, "org-1", "user-1", "alias-1", "provider-msg-1", now); err == nil {
		t.Fatalf("expected zero data retention to refuse mail task decision issuance")
	}
}

func TestIssueMailTaskThreadDecisionRefusesWithoutThreadCreateEntitlement(t *testing.T) {
	signer, _ := testDecisionKeys(t)
	now := time.Date(2026, time.September, 28, 12, 0, 0, 0, time.UTC)
	policy := validMailTaskEffectPolicy("org-1")
	policy.ThreadCreateEntitled = false

	if _, err := IssueMailTaskThreadDecision(policy, signer, "org-1", "user-1", "alias-1", "provider-msg-1", now); err == nil {
		t.Fatalf("expected missing ThreadCreateEntitled to refuse mail task decision issuance")
	}
}

func TestIssueMailTaskThreadDecisionRefusesAMismatchedEffectPolicyOrg(t *testing.T) {
	signer, _ := testDecisionKeys(t)
	now := time.Date(2026, time.September, 28, 12, 0, 0, 0, time.UTC)
	policy := validMailTaskEffectPolicy("org-other")

	if _, err := IssueMailTaskThreadDecision(policy, signer, "org-1", "user-1", "alias-1", "provider-msg-1", now); err == nil {
		t.Fatalf("expected an effect policy for a different org to be refused")
	}
}

func TestVerifyMailTaskThreadDecisionRejectsExpiredOrNotYetValid(t *testing.T) {
	signer, verifier := testDecisionKeys(t)
	now := time.Date(2026, time.September, 28, 12, 0, 0, 0, time.UTC)
	token, err := IssueMailTaskThreadDecision(validMailTaskEffectPolicy("org-1"), signer, "org-1", "user-1", "alias-1", "provider-msg-1", now)
	if err != nil {
		t.Fatalf("IssueMailTaskThreadDecision: %v", err)
	}

	if _, err := VerifyMailTaskThreadDecision(verifier, token, now.Add(10*time.Minute)); err == nil {
		t.Fatalf("expected an expired decision to be rejected")
	}
	if _, err := VerifyMailTaskThreadDecision(verifier, token, now.Add(-10*time.Minute)); err == nil {
		t.Fatalf("expected a not-yet-issued decision to be rejected")
	}
}

func TestVerifyMailTaskThreadDecisionRejectsAWrongSigningKey(t *testing.T) {
	signer, _ := testDecisionKeys(t)
	_, otherVerifier := testDecisionKeys(t)
	now := time.Date(2026, time.September, 28, 12, 0, 0, 0, time.UTC)
	token, err := IssueMailTaskThreadDecision(validMailTaskEffectPolicy("org-1"), signer, "org-1", "user-1", "alias-1", "provider-msg-1", now)
	if err != nil {
		t.Fatalf("IssueMailTaskThreadDecision: %v", err)
	}

	if _, err := VerifyMailTaskThreadDecision(otherVerifier, token, now); err == nil {
		t.Fatalf("expected an untrusted verification key to be rejected")
	}
}

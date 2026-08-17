package spaces

import (
	"crypto/ed25519"
	"strings"
	"testing"
	"time"
)

func validRunActionAuthority() RunActionAuthority {
	return RunActionAuthority{
		RunID:                      "run-1",
		OrgID:                      "org-1",
		SubjectID:                  "user-1",
		ThreadID:                   "thread-1",
		SpaceRef:                   "space-personal",
		RecipientAudienceRef:       "audience:space-personal:2",
		RecipientAudienceRevision:  2,
		RecipientAudienceHash:      "sha256:test-audience",
		PrivacyPolicyRef:           "privacy:org-1:5",
		RunContextAuthorizationRef: "control:space-personal:thread-create:7",
		AuthorityRevision:          7,
		RunStatus:                  "running",
	}
}

func validRunActionRequest() RunActionDecisionRequest {
	return RunActionDecisionRequest{
		ActionID:         "tickets.create",
		ActionSchemaHash: "sha256:" + strings.Repeat("a", 64),
		PayloadDigest:    "sha256:" + strings.Repeat("b", 64),
		IdempotencyKey:   "run-1:tickets.create:1",
		DecisionRef:      "decision-1",
		Nonce:            "nonce-1",
	}
}

func TestIssueRunActionDecisionIsTargetBoundWithoutWideningThreadAuthority(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	authority := validRunActionAuthority()
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)

	decision, err := IssueRunActionDecision(evidence, authority, validRunActionRequest(), now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision: %v", err)
	}
	if decision.ServiceAudience != ticketsCreateServiceAudience ||
		decision.ActionID != "tickets.create" ||
		decision.RunID != authority.RunID ||
		decision.ThreadID != authority.ThreadID {
		t.Fatalf("unexpected target action decision: %#v", decision)
	}
	if decision.RunContextAuthorizationRef != authority.RunContextAuthorizationRef {
		t.Fatalf("run context authorization changed: %#v", decision)
	}
	if strings.Contains(strings.ToLower(strings.Join(decision.Permissions, ",")), "resource") {
		t.Fatalf("thread context must not grant a target resource: %#v", decision)
	}
	if err := decision.Validate(); err != nil {
		t.Fatalf("issued run action decision did not validate: %v", err)
	}
}

func TestIssueRunActionDecisionFailsClosedOnStaleOrIneligibleAuthority(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	for name, mutate := range map[string]func(*PersonalThreadDecisionEvidence, *RunActionAuthority, *RunActionDecisionRequest){
		"agent action entitlement disabled": func(e *PersonalThreadDecisionEvidence, _ *RunActionAuthority, _ *RunActionDecisionRequest) {
			e.AgentActionEntitled = false
		},
		"zero retention": func(e *PersonalThreadDecisionEvidence, _ *RunActionAuthority, _ *RunActionDecisionRequest) {
			e.Privacy.ZeroDataRetention = true
		},
		"stale authority revision": func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority, _ *RunActionDecisionRequest) {
			a.AuthorityRevision++
		},
		"different recipient audience": func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority, _ *RunActionDecisionRequest) {
			a.RecipientAudienceHash = "sha256:other-audience"
		},
		"thread authorization cannot be substituted": func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority, _ *RunActionDecisionRequest) {
			a.RunContextAuthorizationRef = "control:space-personal:retrieval-read:7"
		},
		"unknown action": func(_ *PersonalThreadDecisionEvidence, _ *RunActionAuthority, r *RunActionDecisionRequest) {
			r.ActionID = "users.delete"
		},
		"invalid action schema": func(_ *PersonalThreadDecisionEvidence, _ *RunActionAuthority, r *RunActionDecisionRequest) {
			r.ActionSchemaHash = "sha256:not-a-digest"
		},
		"oversized idempotency key": func(_ *PersonalThreadDecisionEvidence, _ *RunActionAuthority, r *RunActionDecisionRequest) {
			r.IdempotencyKey = strings.Repeat("a", 201)
		},
	} {
		t.Run(name, func(t *testing.T) {
			evidence := validPersonalThreadEvidence()
			evidence.AgentActionEntitled = true
			evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
			authority := validRunActionAuthority()
			request := validRunActionRequest()
			mutate(&evidence, &authority, &request)
			if _, err := IssueRunActionDecision(evidence, authority, request, now); err == nil {
				t.Fatal("ineligible or stale run authority unexpectedly issued a target decision")
			}
		})
	}
}

func TestValidateCurrentRunActionDecisionRejectsSupersededControlFacts(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	decision, err := IssueRunActionDecision(evidence, validRunActionAuthority(), validRunActionRequest(), now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision() error = %v", err)
	}
	if err := ValidateCurrentRunActionDecision(evidence, decision); err != nil {
		t.Fatalf("ValidateCurrentRunActionDecision() error = %v", err)
	}

	for name, mutate := range map[string]func(*PersonalThreadDecisionEvidence){
		"recipient audience changed": func(e *PersonalThreadDecisionEvidence) {
			e.RecipientAudienceHash = "sha256:changed-audience"
			e.Membership.Revisions.Authority++
			e.Membership.Revisions.RecipientAudience++
		},
		"privacy policy changed": func(e *PersonalThreadDecisionEvidence) {
			e.Privacy.PolicyRef = "privacy:org-1:changed"
			e.Membership.Revisions.Authority++
			e.Membership.Revisions.Privacy++
		},
		"agent action entitlement revoked": func(e *PersonalThreadDecisionEvidence) {
			e.AgentActionEntitled = false
			e.Membership.Revisions.Authority++
			e.Membership.Revisions.Entitlement++
		},
	} {
		t.Run(name, func(t *testing.T) {
			current := evidence
			mutate(&current)
			if err := ValidateCurrentRunActionDecision(current, decision); err == nil {
				t.Fatal("superseded Control facts unexpectedly authorized the old decision")
			}
		})
	}
}

func TestVerifyRunActionDecisionRejectsTamperingAndExpiry(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	decision, err := IssueRunActionDecision(evidence, validRunActionAuthority(), validRunActionRequest(), now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision() error = %v", err)
	}
	key := SigningKey{ID: "control-test", PrivateKey: ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))}
	token, err := SignRunActionDecision(key, decision)
	if err != nil {
		t.Fatalf("SignRunActionDecision() error = %v", err)
	}
	if _, err := VerifyRunActionDecision(key, token, now.Add(time.Minute)); err != nil {
		t.Fatalf("VerifyRunActionDecision() error = %v", err)
	}
	if _, err := VerifyRunActionDecision(key, token+"x", now.Add(time.Minute)); err == nil {
		t.Fatal("tampered decision token accepted")
	}
	if _, err := VerifyRunActionDecision(key, token, decision.ExpiresAt.Add(time.Second)); err == nil {
		t.Fatal("expired decision token accepted")
	}
}

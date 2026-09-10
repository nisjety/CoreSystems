package spaces

import (
	"strings"
	"testing"
	"time"
)

func validSpaceCapabilityIntent() SpaceCapabilityIntent {
	return SpaceCapabilityIntent{
		OrgID: "org-1", SpaceRef: "space-personal", SubjectID: "user-1",
		BackendID:      "bubblewrap-host-a1",
		ProfileDigest:  "sha256:" + strings.Repeat("a", 64),
		Persistence:    "ephemeral",
		Processes:      "bounded_oneshot",
		Backup:         false,
		Egress:         "disabled_by_default",
		CredentialMode: "credential_free",
		IdempotencyKey: "bubblewrap-host-a1:sha256:" + strings.Repeat("a", 64),
	}
}

func TestIssueSpaceCapabilityDecisionBindsOneBackendClaim(t *testing.T) {
	now := time.Date(2026, time.September, 11, 10, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.SandboxCapabilityEntitled = true
	intent := validSpaceCapabilityIntent()

	decision, err := IssueSpaceCapabilityDecision(evidence, intent, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("IssueSpaceCapabilityDecision: %v", err)
	}
	if decision.ActionID != spaceCapabilityAction || decision.ActionSchemaHash != spaceCapabilitySchema || decision.ServiceAudience != spaceCapabilityAudience {
		t.Fatalf("unexpected space capability contract: %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "space:sandbox:use" {
		t.Fatalf("space capability permission with default-disabled egress: %#v", decision.Permissions)
	}
	if decision.ExpiresAt != now.Add(personalDecisionLifetime) {
		t.Fatalf("unexpected expiry: %s", decision.ExpiresAt)
	}
	// ZDR does not disqualify this decision, unlike schedule fire.
	evidence.Privacy.ZeroDataRetention = true
	if _, err := IssueSpaceCapabilityDecision(evidence, intent, "decision-1", "nonce-1", now); err != nil {
		t.Fatalf("ZDR space capability decision was rejected: %v", err)
	}

	changed := intent
	changed.BackendID = "bubblewrap-host-a2"
	changed.IdempotencyKey = "bubblewrap-host-a2:sha256:" + strings.Repeat("a", 64)
	second, err := IssueSpaceCapabilityDecision(evidence, changed, "decision-2", "nonce-2", now)
	if err != nil {
		t.Fatalf("second IssueSpaceCapabilityDecision: %v", err)
	}
	if decision.PayloadDigest == second.PayloadDigest {
		t.Fatal("a decision for one backend must not authorize a claim pinned to another backend")
	}
}

func TestIssueSpaceCapabilityDecisionGrantsEgressPermissionOnlyWhenClaimed(t *testing.T) {
	now := time.Now().UTC()
	evidence := validPersonalThreadEvidence()
	evidence.SandboxCapabilityEntitled = true
	intent := validSpaceCapabilityIntent()
	intent.Egress = "allow_domains"

	decision, err := IssueSpaceCapabilityDecision(evidence, intent, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("IssueSpaceCapabilityDecision: %v", err)
	}
	found := false
	for _, permission := range decision.Permissions {
		if permission == "space:egress" {
			found = true
		}
	}
	if !found {
		t.Fatalf("egress-capable claim did not grant space:egress: %#v", decision.Permissions)
	}
}

func TestIssueSpaceCapabilityDecisionFailsClosedWithoutEntitlementOrMatchingIntent(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	intent := validSpaceCapabilityIntent()
	if _, err := IssueSpaceCapabilityDecision(evidence, intent, "decision-1", "nonce-1", time.Now().UTC()); err == nil {
		t.Fatal("space capability without the explicit entitlement was authorized")
	}

	evidence.SandboxCapabilityEntitled = true
	intent.SubjectID = "forged-user"
	if _, err := IssueSpaceCapabilityDecision(evidence, intent, "decision-1", "nonce-1", time.Now().UTC()); err == nil {
		t.Fatal("mismatched space capability intent was authorized")
	}
}

func TestIssueSpaceCapabilityDecisionRejectsInvalidProfileDigest(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.SandboxCapabilityEntitled = true
	intent := validSpaceCapabilityIntent()
	intent.ProfileDigest = "not-a-digest"
	if _, err := IssueSpaceCapabilityDecision(evidence, intent, "decision-1", "nonce-1", time.Now().UTC()); err == nil {
		t.Fatal("malformed profile digest was authorized")
	}
}

func TestIssueSpaceCapabilityDecisionRejectsViewerRole(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.SandboxCapabilityEntitled = true
	evidence.Membership.Role = "viewer"
	if _, err := IssueSpaceCapabilityDecision(evidence, validSpaceCapabilityIntent(), "decision-1", "nonce-1", time.Now().UTC()); err == nil {
		t.Fatal("viewer role acquired a sandbox capability")
	}
}

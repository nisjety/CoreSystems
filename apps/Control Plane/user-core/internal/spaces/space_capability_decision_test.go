package spaces

import (
	"slices"
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

// TestIssueSpaceCapabilityDecisionGrantsProcessesOnlyWhenClaimedAndEntitled
// is S4.2's authority rule in full. The third case is the one that matters
// most and is deliberately NOT symmetric with egress: a backend that can
// host background processes, asking for a Space that is not entitled to
// them, still gets a valid decision — it is a perfectly good bounded
// one-shot substrate for that Space. Only the permission is withheld, and
// sandbox-manager refuses the process RPCs alone.
func TestIssueSpaceCapabilityDecisionGrantsProcessesOnlyWhenClaimedAndEntitled(t *testing.T) {
	now := time.Date(2026, time.September, 13, 10, 0, 0, 0, time.UTC)
	cases := []struct {
		name      string
		processes string
		entitled  bool
		want      bool
	}{
		{name: "claimed and entitled", processes: ProcessesBackgroundRegistry, entitled: true, want: true},
		{name: "claimed but not entitled", processes: ProcessesBackgroundRegistry, entitled: false, want: false},
		{name: "entitled but the backend only runs one-shot children", processes: ProcessesBoundedOneshot, entitled: true, want: false},
		{name: "neither", processes: ProcessesBoundedOneshot, entitled: false, want: false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			evidence := validPersonalThreadEvidence()
			evidence.SandboxCapabilityEntitled = true
			evidence.ProcessRegistryEntitled = tc.entitled
			intent := validSpaceCapabilityIntent()
			intent.Processes = tc.processes

			decision, err := IssueSpaceCapabilityDecision(evidence, intent, "decision-1", "nonce-1", now)
			if err != nil {
				t.Fatalf("a process-capable claim must still yield a usable decision: %v", err)
			}
			granted := false
			for _, permission := range decision.Permissions {
				if permission == "space:processes" {
					granted = true
				}
			}
			if granted != tc.want {
				t.Fatalf("space:processes granted = %v, want %v (permissions %#v)", granted, tc.want, decision.Permissions)
			}
			// The lease itself is never withheld over this.
			if !hasPermissionInDecision(decision, "space:sandbox:use") {
				t.Fatalf("the sandbox permission was withheld: %#v", decision.Permissions)
			}
		})
	}
}

// TestProcessRegistryEntitlementIsIndependentOfSandboxEntitlement: a Space
// entitled to background processes but not to a sandbox at all cannot use
// the first as a way around the second.
func TestProcessRegistryEntitlementIsIndependentOfSandboxEntitlement(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.SandboxCapabilityEntitled = false
	evidence.ProcessRegistryEntitled = true
	intent := validSpaceCapabilityIntent()
	intent.Processes = ProcessesBackgroundRegistry

	if _, err := IssueSpaceCapabilityDecision(evidence, intent, "decision-1", "nonce-1", time.Now().UTC()); err == nil {
		t.Fatal("process entitlement must not substitute for the sandbox entitlement")
	}
}

// TestSpaceCapabilityIntentRejectsAnUnrecognizedProcessesClaim closes the
// gap the S3.2 design's §1 promised and never built: `processes` was
// free-form and checked only for non-emptiness, so Control would sign a
// decision for a backend claiming anything at all. "isolated" is not a
// hypothetical — it is the value sandbox-manager's own verifier fixture
// used, which is how nobody noticed the claim was never validated.
func TestSpaceCapabilityIntentRejectsAnUnrecognizedProcessesClaim(t *testing.T) {
	for _, claim := range []string{"isolated", "durable", "BACKGROUND_REGISTRY", "bounded oneshot", " "} {
		intent := validSpaceCapabilityIntent()
		intent.Processes = claim
		if err := intent.Validate(); err == nil {
			t.Fatalf("processes claim %q was accepted", claim)
		}
	}
	for _, claim := range []string{ProcessesBoundedOneshot, ProcessesBackgroundRegistry} {
		intent := validSpaceCapabilityIntent()
		intent.Processes = claim
		if err := intent.Validate(); err != nil {
			t.Fatalf("processes claim %q was rejected: %v", claim, err)
		}
	}
}

// TestTheProcessesClaimIsBoundIntoThePayloadDigest: the claim is only
// meaningful because sandbox-manager recomputes this digest from the
// plaintext claims it receives. Two different claims must not produce the
// same digest, or a process-capable profile could be swapped in under a
// signature issued for a one-shot one.
func TestTheProcessesClaimIsBoundIntoThePayloadDigest(t *testing.T) {
	now := time.Date(2026, time.September, 13, 10, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.SandboxCapabilityEntitled = true
	evidence.ProcessRegistryEntitled = true

	oneshot := validSpaceCapabilityIntent()
	oneshot.Processes = ProcessesBoundedOneshot
	background := validSpaceCapabilityIntent()
	background.Processes = ProcessesBackgroundRegistry

	first, err := IssueSpaceCapabilityDecision(evidence, oneshot, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("IssueSpaceCapabilityDecision: %v", err)
	}
	second, err := IssueSpaceCapabilityDecision(evidence, background, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("IssueSpaceCapabilityDecision: %v", err)
	}
	if first.PayloadDigest == second.PayloadDigest {
		t.Fatal("the processes claim is not bound into the payload digest")
	}
}

func hasPermissionInDecision(decision Decision, permission string) bool {
	return slices.Contains(decision.Permissions, permission)
}

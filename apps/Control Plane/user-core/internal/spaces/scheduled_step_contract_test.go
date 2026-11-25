package spaces

import (
	"errors"
	"testing"
	"time"
)

// F-f10-27: a scheduled step's stored-consent contract binding is read only from the
// prepared Session Core authority. A request that claims a binding is ignored, and a
// legacy step keeps its v1 digest.

const (
	testContractDigest = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
	testClaimedDigest  = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
	// legacyV1StepPayloadDigest is the v1 payload digest of normalizedScheduledStepIntent
	// under validPersonalThreadEvidence (both entitlements on, the decision time
	// 2026-08-16T00:00Z). It was captured from the code before the contract binding was
	// read from the prepared authority: a legacy step must keep it byte for byte.
	legacyV1StepPayloadDigest = "sha256:a80758f3a09eb4bd9b5cb15e4d7aef43730081c60fc0e28f558b0efcdce6e105"
)

func contractTestEvidence() PersonalThreadDecisionEvidence {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	evidence.AgentActionEntitled = true
	return evidence
}

func TestBindScheduledStepContractTakesTheBindingOnlyFromPrepared(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	prepared := ScheduledStepAuthority{ContractDigest: testContractDigest, ContractExpiresAt: now.Add(time.Hour)}

	// The intent a request could carry claims a different digest and a later expiry.
	claimed := normalizedScheduledStepIntent()
	claimed.ContractDigest = testClaimedDigest
	claimed.ContractExpiresAt = now.Add(2 * time.Hour)

	bound, err := BindScheduledStepContract(claimed, prepared, now)
	if err != nil {
		t.Fatalf("BindScheduledStepContract: %v", err)
	}
	if bound.ContractDigest != testContractDigest || !bound.ContractExpiresAt.Equal(prepared.ContractExpiresAt) {
		t.Fatalf("bound contract = %q at %s, want the prepared %q at %s",
			bound.ContractDigest, bound.ContractExpiresAt, testContractDigest, prepared.ContractExpiresAt)
	}

	decision, err := IssueScheduledStepDecision(contractTestEvidence(), bound, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("contract-bound step decision: %v", err)
	}
	if decision.ActionSchemaHash != scheduledStepSchemaContract || decision.ContractDigest != testContractDigest ||
		decision.ContractExpiresAt == nil || !decision.ContractExpiresAt.Equal(prepared.ContractExpiresAt) {
		t.Fatalf("decision does not carry the prepared contract: %#v", decision)
	}
}

func TestBindScheduledStepContractKeepsALegacyStepOnTheV1Binding(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	// A prepared step with no contract clears any contract the intent claims.
	claimed := normalizedScheduledStepIntent()
	claimed.ContractDigest = testClaimedDigest
	claimed.ContractExpiresAt = now.Add(time.Hour)

	legacy, err := BindScheduledStepContract(claimed, ScheduledStepAuthority{}, now)
	if err != nil {
		t.Fatalf("legacy BindScheduledStepContract: %v", err)
	}
	if legacy.hasContract() || legacy.ContractDigest != "" || !legacy.ContractExpiresAt.IsZero() {
		t.Fatalf("legacy step kept a claimed contract: %#v", legacy)
	}

	decision, err := IssueScheduledStepDecision(contractTestEvidence(), legacy, "decision-2", "nonce-2", now)
	if err != nil {
		t.Fatalf("legacy step decision: %v", err)
	}
	if decision.ActionSchemaHash != scheduledStepSchema || decision.ContractDigest != "" || decision.ContractExpiresAt != nil {
		t.Fatalf("legacy step decision must keep the v1 binding: %#v", decision)
	}
	if decision.PayloadDigest != legacyV1StepPayloadDigest {
		t.Fatalf("legacy v1 payload digest changed: got %s want %s", decision.PayloadDigest, legacyV1StepPayloadDigest)
	}
}

func TestBindScheduledStepContractRefusesAnExpiredContract(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	for name, expiresAt := range map[string]time.Time{
		"expired an hour ago":     now.Add(-time.Hour),
		"expired at this instant": now,
	} {
		prepared := ScheduledStepAuthority{ContractDigest: testContractDigest, ContractExpiresAt: expiresAt}
		if _, err := BindScheduledStepContract(normalizedScheduledStepIntent(), prepared, now); !errors.Is(err, ErrScheduledContractExpired) {
			t.Fatalf("%s: err=%v, want ErrScheduledContractExpired", name, err)
		}
	}
}

func TestBindScheduledStepContractRefusesAHalfBoundOrMalformedContract(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	for name, prepared := range map[string]ScheduledStepAuthority{
		"a digest without an expiry": {ContractDigest: testContractDigest},
		"a malformed digest":         {ContractDigest: "sha256:short", ContractExpiresAt: now.Add(time.Hour)},
	} {
		_, err := BindScheduledStepContract(normalizedScheduledStepIntent(), prepared, now)
		if err == nil {
			t.Fatalf("%s: a contract that is not bound whole was accepted", name)
		}
		if errors.Is(err, ErrScheduledContractExpired) {
			t.Fatalf("%s: refused as expired instead of invalid: %v", name, err)
		}
	}
}

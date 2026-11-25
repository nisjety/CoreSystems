package spaces

import (
	"errors"
	"strings"
	"testing"
	"time"
)

// F-f10-27 rel1: the run-execution decision binds the stored-consent contract only from
// the prepared Session Core run. A contract the Orchestrator body claims is never a
// binding, so the decision cannot be anchored to a contract Session Core did not prepare.

func preparedScheduledRun(contractDigest string, expiresAt time.Time) ScheduledRunAuthority {
	intent := validScheduledRunIntent()
	return ScheduledRunAuthority{
		RunID: intent.TaskID, ThreadID: "thread-1", OrgID: intent.OrgID, SubjectID: intent.SubjectID,
		SpaceRef: intent.SpaceRef, ScheduleID: intent.ScheduleID, FireKey: intent.FireKey,
		TemplateDigest: intent.TemplateDigest, IdempotencyKey: intent.IdempotencyKey,
		ContractDigest: contractDigest, ContractExpiresAt: expiresAt,
	}
}

func TestBindScheduledRunContractTakesTheContractOnlyFromPrepared(t *testing.T) {
	now := time.Date(2026, 8, 14, 0, 0, 0, 0, time.UTC)
	expiresAt := now.Add(time.Hour)
	prepared := preparedScheduledRun(validScheduledContractDigest, expiresAt)

	// A claim that names another digest and a later expiry is overwritten by the prepared contract.
	claimed := contractBoundRunIntent(now.Add(3 * time.Hour))
	claimed.ContractDigest = "sha256:" + strings.Repeat("d", 64)

	bound, err := BindScheduledRunContract(claimed, prepared, now)
	if err != nil {
		t.Fatalf("BindScheduledRunContract: %v", err)
	}
	if bound.ContractDigest != validScheduledContractDigest || !bound.ContractExpiresAt.Equal(expiresAt) {
		t.Fatalf("bound contract = %q at %s, want the prepared %q at %s",
			bound.ContractDigest, bound.ContractExpiresAt, validScheduledContractDigest, expiresAt)
	}
}

func TestBindScheduledRunContractRefusesAnExpiredPreparedContract(t *testing.T) {
	now := time.Date(2026, 8, 14, 0, 0, 0, 0, time.UTC)
	for name, expiresAt := range map[string]time.Time{
		"expires now":          now,
		"expired a minute ago": now.Add(-time.Minute),
	} {
		prepared := preparedScheduledRun(validScheduledContractDigest, expiresAt)
		if _, err := BindScheduledRunContract(validScheduledRunIntent(), prepared, now); !errors.Is(err, ErrScheduledContractExpired) {
			t.Fatalf("%s: err = %v, want ErrScheduledContractExpired", name, err)
		}
	}
}

func TestBindScheduledRunContractKeepsALegacyRunOnTheV1Binding(t *testing.T) {
	now := time.Date(2026, 8, 14, 0, 0, 0, 0, time.UTC)
	prepared := preparedScheduledRun("", time.Time{})

	// Even a claim is dropped when Session Core prepared no contract for the run.
	claimed := contractBoundRunIntent(now.Add(time.Hour))
	bound, err := BindScheduledRunContract(claimed, prepared, now)
	if err != nil {
		t.Fatalf("legacy bind: %v", err)
	}
	if bound.ContractDigest != "" || !bound.ContractExpiresAt.IsZero() {
		t.Fatalf("a legacy run gained a contract binding: %#v", bound)
	}

	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	decision, err := IssueScheduledRunExecutionDecision(evidence, bound, "thread-1", "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("legacy execution decision: %v", err)
	}
	if decision.ActionSchemaHash != "sha256:space-scheduled-run-execute-v1" || decision.ContractDigest != "" || decision.ContractExpiresAt != nil {
		t.Fatalf("a legacy run must keep the v1 execution binding: %#v", decision)
	}
}

func TestExecutionDecisionFromAPreparedContractCarriesThePreparedDigest(t *testing.T) {
	now := time.Date(2026, 8, 14, 0, 0, 0, 0, time.UTC)
	expiresAt := now.Add(time.Hour)
	prepared := preparedScheduledRun(validScheduledContractDigest, expiresAt)
	bound, err := BindScheduledRunContract(validScheduledRunIntent(), prepared, now)
	if err != nil {
		t.Fatalf("BindScheduledRunContract: %v", err)
	}

	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	decision, err := IssueScheduledRunExecutionDecision(evidence, bound, "thread-1", "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("contract-bound execution decision: %v", err)
	}
	if decision.ActionSchemaHash != "sha256:space-scheduled-run-execute-v2" {
		t.Fatalf("a prepared contract must use execution schema v2: %q", decision.ActionSchemaHash)
	}
	if decision.ContractDigest != validScheduledContractDigest || decision.ContractExpiresAt == nil || !decision.ContractExpiresAt.Equal(expiresAt) {
		t.Fatalf("the decision must carry the prepared digest and expiry: %#v", decision)
	}
}

func TestScheduledRunAuthorityValidateRefusesAnIncompleteProjection(t *testing.T) {
	now := time.Date(2026, 8, 14, 0, 0, 0, 0, time.UTC)
	if err := preparedScheduledRun("", time.Time{}).Validate(); err != nil {
		t.Fatalf("a complete legacy projection must validate: %v", err)
	}
	if err := preparedScheduledRun(validScheduledContractDigest, now.Add(time.Hour)).Validate(); err != nil {
		t.Fatalf("a complete contract-bound projection must validate: %v", err)
	}

	noThread := preparedScheduledRun("", time.Time{})
	noThread.ThreadID = " "
	if err := noThread.Validate(); err == nil {
		t.Fatal("a prepared run without a thread must not validate")
	}
	digestOnly := preparedScheduledRun(validScheduledContractDigest, time.Time{})
	if err := digestOnly.Validate(); err == nil {
		t.Fatal("a digest without an expiry is a half-bound contract and must not validate")
	}
	expiryOnly := preparedScheduledRun("", now.Add(time.Hour))
	if err := expiryOnly.Validate(); err == nil {
		t.Fatal("an expiry without a digest is a half-bound contract and must not validate")
	}
}

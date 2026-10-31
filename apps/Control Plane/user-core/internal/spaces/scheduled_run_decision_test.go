package spaces

import (
	"testing"
	"time"
)

func validScheduledRunIntent() ScheduledRunIntent {
	return ScheduledRunIntent{
		OrgID:          "org-1",
		SpaceRef:       "space-personal",
		SubjectID:      "user-1",
		ScheduleID:     "schedule-1",
		FireKey:        "2026-08-14T00:00:00Z",
		TaskID:         "task-1",
		TemplateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		IdempotencyKey: "schedule-1/2026-08-14T00:00:00Z",
	}
}

const validScheduledContractDigest = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"

func contractBoundRunIntent(expiresAt time.Time) ScheduledRunIntent {
	intent := validScheduledRunIntent()
	intent.ContractDigest = validScheduledContractDigest
	intent.ContractExpiresAt = expiresAt
	return intent
}

func TestIssueScheduledRunExecutionDecisionBindsContractDigestAndExpiry(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	now := time.Date(2026, 8, 14, 0, 0, 0, 0, time.UTC)
	expiresAt := now.Add(time.Hour)

	legacy, err := IssueScheduledRunExecutionDecision(evidence, validScheduledRunIntent(), "thread-1", "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("legacy execution decision: %v", err)
	}
	if legacy.ActionSchemaHash != "sha256:space-scheduled-run-execute-v1" || legacy.ContractDigest != "" || legacy.ContractExpiresAt != nil {
		t.Fatalf("a schedule without a contract must keep the v1 binding: %#v", legacy)
	}

	bound, err := IssueScheduledRunExecutionDecision(evidence, contractBoundRunIntent(expiresAt), "thread-1", "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("contract-bound execution decision: %v", err)
	}
	if bound.ActionSchemaHash != "sha256:space-scheduled-run-execute-v2" {
		t.Fatalf("contract-bound execution must use schema v2: %q", bound.ActionSchemaHash)
	}
	if bound.ContractDigest != validScheduledContractDigest || bound.ContractExpiresAt == nil || !bound.ContractExpiresAt.Equal(expiresAt) {
		t.Fatalf("contract digest and expiry must be carried on the decision: %#v", bound)
	}
	if bound.PayloadDigest == legacy.PayloadDigest {
		t.Fatal("the contract must change the execution payload digest")
	}
}

func TestIssueScheduledRunDecisionsRefuseExpiredContract(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	now := time.Date(2026, 8, 14, 0, 0, 0, 0, time.UTC)
	expired := contractBoundRunIntent(now)

	if _, err := IssueScheduledRunExecutionDecision(evidence, expired, "thread-1", "decision-1", "nonce-1", now); err == nil {
		t.Fatal("an expired contract must not authorize a scheduled-run execution")
	}
	if _, err := IssueScheduledRunDecision(evidence, expired, "decision-1", "nonce-1", now); err == nil {
		t.Fatal("an expired contract must not authorize scheduled-run preparation")
	}
}

func TestIssueScheduledRunPreparationDoesNotBindContract(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	now := time.Date(2026, 8, 14, 0, 0, 0, 0, time.UTC)

	legacy, err := IssueScheduledRunDecision(evidence, validScheduledRunIntent(), "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("legacy preparation decision: %v", err)
	}
	prepared, err := IssueScheduledRunDecision(evidence, contractBoundRunIntent(now.Add(time.Hour)), "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("live-contract preparation decision: %v", err)
	}
	if prepared.ActionSchemaHash != "sha256:space-scheduled-run-v1" || prepared.ContractDigest != "" || prepared.ContractExpiresAt != nil {
		t.Fatalf("preparation must stay on the v1 binding: %#v", prepared)
	}
	if prepared.PayloadDigest != legacy.PayloadDigest {
		t.Fatal("preparation payload digest must not depend on the contract")
	}
}

func TestIssueScheduledRunDecisionBindsOneTaskAndDeterministicThread(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	now := time.Date(2026, 8, 14, 0, 0, 0, 0, time.UTC)
	intent := validScheduledRunIntent()

	decision, err := IssueScheduledRunDecision(evidence, intent, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("IssueScheduledRunDecision: %v", err)
	}
	if decision.ActionID != "model.schedule.run" || decision.ActionSchemaHash != "sha256:space-scheduled-run-v1" {
		t.Fatalf("unexpected action contract: %#v", decision)
	}
	if decision.ServiceAudience != "model-plane-capability-core" || !matchesOneOf("schedule:run", decision.Permissions...) {
		t.Fatalf("unexpected audience/permission: %#v", decision)
	}

	changed := intent
	changed.TaskID = "task-2"
	second, err := IssueScheduledRunDecision(evidence, changed, "decision-2", "nonce-2", now)
	if err != nil {
		t.Fatalf("changed task decision: %v", err)
	}
	if decision.PayloadDigest == second.PayloadDigest {
		t.Fatal("one-fire decision must bind the exact task/run id")
	}
	if intent.SystemThreadKey() != "schedule/schedule-1/2026-08-14T00:00:00Z" {
		t.Fatalf("system thread key = %q", intent.SystemThreadKey())
	}
}

func TestIssueScheduledRunDecisionFailsClosedOnRevokedOrMismatchedAuthority(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	intent := validScheduledRunIntent()
	if _, err := IssueScheduledRunDecision(evidence, intent, "decision-1", "nonce-1", time.Now().UTC()); err == nil {
		t.Fatal("missing schedule entitlement must be denied")
	}
	evidence.ScheduleFireEntitled = true
	intent.SubjectID = "another-user"
	if _, err := IssueScheduledRunDecision(evidence, intent, "decision-1", "nonce-1", time.Now().UTC()); err == nil {
		t.Fatal("mismatched subject must be denied")
	}
}

func TestIssueScheduledRunExecutionDecisionIsDistinctAndBoundToPreparedThread(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	intent := validScheduledRunIntent()
	now := time.Date(2026, 8, 15, 0, 0, 0, 0, time.UTC)

	decision, err := IssueScheduledRunExecutionDecision(
		evidence, intent, "thread-scheduled-1", "decision-execute", "nonce-execute", now,
	)
	if err != nil {
		t.Fatalf("IssueScheduledRunExecutionDecision: %v", err)
	}
	if decision.ActionID != scheduledRunExecutionAction ||
		decision.ActionSchemaHash != scheduledRunExecutionSchema ||
		decision.ServiceAudience != scheduledRunExecutionAudience ||
		!matchesOneOf("schedule:execute", decision.Permissions...) {
		t.Fatalf("unexpected execution decision: %#v", decision)
	}
	preparation, err := IssueScheduledRunDecision(evidence, intent, "decision-prepare", "nonce-prepare", now)
	if err != nil {
		t.Fatalf("IssueScheduledRunDecision: %v", err)
	}
	if decision.PayloadDigest == preparation.PayloadDigest {
		t.Fatal("execution decision must not be interchangeable with preparation")
	}
	otherThread, err := IssueScheduledRunExecutionDecision(
		evidence, intent, "thread-scheduled-2", "decision-execute-2", "nonce-execute-2", now,
	)
	if err != nil {
		t.Fatalf("IssueScheduledRunExecutionDecision for changed thread: %v", err)
	}
	if decision.PayloadDigest == otherThread.PayloadDigest {
		t.Fatal("execution decision must bind the exact prepared thread")
	}
}

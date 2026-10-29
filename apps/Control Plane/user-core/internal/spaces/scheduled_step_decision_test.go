package spaces

import (
	"testing"
	"time"
)

func normalizedScheduledStepIntent() ScheduledStepIntent {
	return ScheduledStepIntent{
		OrgID: "org-1", SpaceRef: "space-personal", SubjectID: "user-1",
		RunID: "run-1", ThreadID: "thread-1", ScheduleID: "schedule-1",
		FireKey:        "fire-1",
		TemplateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		StepID:         "step-1", StepIndex: 0,
		PolicyDigest:   "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		IdempotencyKey: "run-1:step-1",
	}
}

func TestIssueScheduledStepDecisionBindsExactTurnAndRequiresBothEntitlements(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	evidence.AgentActionEntitled = true
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	intent := normalizedScheduledStepIntent()

	decision, err := IssueScheduledStepDecision(evidence, intent, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("IssueScheduledStepDecision: %v", err)
	}
	if decision.ActionID != scheduledStepAction || decision.ServiceAudience != scheduledStepAudience ||
		decision.ActionSchemaHash != scheduledStepSchema || !matchesOneOf("schedule:step", decision.Permissions...) {
		t.Fatalf("unexpected step decision: %#v", decision)
	}

	changed := intent
	changed.StepIndex = 1
	second, err := IssueScheduledStepDecision(evidence, changed, "decision-2", "nonce-2", now)
	if err != nil {
		t.Fatalf("changed step decision: %v", err)
	}
	if decision.PayloadDigest == second.PayloadDigest {
		t.Fatal("step decision must bind the exact step index")
	}

	evidence.AgentActionEntitled = false
	if _, err := IssueScheduledStepDecision(evidence, intent, "decision-3", "nonce-3", now); err == nil {
		t.Fatal("missing agent-action entitlement must fail closed")
	}
}

func TestScheduledStepAuthorityRequiresSessionBoundSubjectAndExactStep(t *testing.T) {
	authority := ScheduledStepAuthority{
		RunID: "run-1", ThreadID: "thread-1", OrgID: "org-1", SubjectID: "user-1", SpaceRef: "space-1",
		ScheduleID: "schedule-1", FireKey: "fire-1",
		TemplateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		PolicyDigest:   "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		StepID:         "run-1:step:0", StepIndex: 0, IdempotencyKey: "fire-1:step:0", RunStatus: "running",
	}
	if err := authority.Validate(); err != nil {
		t.Fatalf("valid prepared authority rejected: %v", err)
	}
	authority.StepID = "other-run:step:0"
	if err := authority.Validate(); err == nil {
		t.Fatal("authority accepted a step id not bound to its run")
	}
}

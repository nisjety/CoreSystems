package spaces

import (
	"strings"
	"testing"
	"time"
)

func validScheduleCreateRequest() ScheduleCreateRequest {
	return ScheduleCreateRequest{
		DecisionRef: "decision-1", ScheduleID: "schedule-1",
		TemplateDigest: "sha256:" + strings.Repeat("b", 64),
		IdempotencyKey: "create-schedule-1", Nonce: "nonce-1",
	}
}

func TestIssueScheduleCreateDecisionIsTemplateAndScheduleBound(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	request := validScheduleCreateRequest()
	now := time.Date(2026, time.August, 13, 11, 0, 0, 0, time.UTC)

	decision, err := IssueScheduleCreateDecision(evidence, request, now)
	if err != nil {
		t.Fatalf("IssueScheduleCreateDecision: %v", err)
	}
	if decision.ActionID != scheduleCreateAction || decision.ActionSchemaHash != scheduleCreateSchema || decision.ServiceAudience != scheduleCreateAudience {
		t.Fatalf("unexpected schedule create contract: %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "cron:create" {
		t.Fatalf("schedule create permission: %#v", decision.Permissions)
	}
	changed := request
	changed.TemplateDigest = "sha256:" + strings.Repeat("c", 64)
	changed.IdempotencyKey = "create-schedule-1-changed"
	second, err := IssueScheduleCreateDecision(evidence, changed, now)
	if err != nil {
		t.Fatalf("second IssueScheduleCreateDecision: %v", err)
	}
	if decision.PayloadDigest == second.PayloadDigest {
		t.Fatal("a create decision must bind the persisted task template")
	}
}

func TestIssueScheduleCreateDecisionFailsClosedWithoutScheduleEntitlement(t *testing.T) {
	if _, err := IssueScheduleCreateDecision(validPersonalThreadEvidence(), validScheduleCreateRequest(), time.Now().UTC()); err == nil {
		t.Fatal("schedule create without explicit schedule entitlement was authorized")
	}
}

func TestIssueScheduleCreateDecisionRejectsZeroDataRetention(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	evidence.Privacy.ZeroDataRetention = true
	if _, err := IssueScheduleCreateDecision(evidence, validScheduleCreateRequest(), time.Now().UTC()); err == nil {
		t.Fatal("ZDR Space schedule creation was authorized")
	}
}

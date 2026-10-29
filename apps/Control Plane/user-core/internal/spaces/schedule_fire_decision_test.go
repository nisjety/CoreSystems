package spaces

import (
	"strings"
	"testing"
	"time"
)

func validScheduleFireIntent() ScheduleFireIntent {
	return ScheduleFireIntent{
		OrgID: "org-1", SpaceRef: "space-personal", SubjectID: "user-1",
		ScheduleID: "schedule-1", FireKey: "2026-08-13T10:00:00Z",
		TemplateDigest: "sha256:" + strings.Repeat("a", 64), IdempotencyKey: "schedule-1:2026-08-13T10:00:00Z",
	}
}

func TestIssueScheduleFireDecisionRefreshesCurrentAuthorityAndBindsOneFire(t *testing.T) {
	now := time.Date(2026, time.August, 13, 10, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	intent := validScheduleFireIntent()

	decision, err := IssueScheduleFireDecision(evidence, intent, "decision-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("IssueScheduleFireDecision: %v", err)
	}
	if decision.ActionID != scheduleFireAction || decision.ActionSchemaHash != scheduleFireSchema || decision.ServiceAudience != scheduleFireAudience {
		t.Fatalf("unexpected schedule fire contract: %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "cron:fire" {
		t.Fatalf("schedule fire permission: %#v", decision.Permissions)
	}
	if decision.ExpiresAt != now.Add(personalDecisionLifetime) {
		t.Fatalf("unexpected expiry: %s", decision.ExpiresAt)
	}

	changed := intent
	changed.FireKey = "2026-08-13T10:05:00Z"
	changed.IdempotencyKey = "schedule-1:2026-08-13T10:05:00Z"
	second, err := IssueScheduleFireDecision(evidence, changed, "decision-2", "nonce-2", now)
	if err != nil {
		t.Fatalf("second IssueScheduleFireDecision: %v", err)
	}
	if decision.PayloadDigest == second.PayloadDigest {
		t.Fatal("a decision for one scheduled fire must not authorize another fire")
	}
}

func TestIssueScheduleFireDecisionFailsClosedWithoutScheduleEntitlementOrMatchingIntent(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	intent := validScheduleFireIntent()
	if _, err := IssueScheduleFireDecision(evidence, intent, "decision-1", "nonce-1", time.Now().UTC()); err == nil {
		t.Fatal("schedule fire without the explicit entitlement was authorized")
	}

	evidence.ScheduleFireEntitled = true
	intent.SubjectID = "forged-user"
	if _, err := IssueScheduleFireDecision(evidence, intent, "decision-1", "nonce-1", time.Now().UTC()); err == nil {
		t.Fatal("mismatched durable schedule intent was authorized")
	}
}

func TestIssueScheduleFireDecisionRejectsZeroDataRetention(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	evidence.Privacy.ZeroDataRetention = true
	if _, err := IssueScheduleFireDecision(evidence, validScheduleFireIntent(), "decision-1", "nonce-1", time.Now().UTC()); err == nil {
		t.Fatal("ZDR Space schedule fire was authorized")
	}
}

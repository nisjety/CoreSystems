package spaces

import (
	"strings"
	"testing"
	"time"
)

func contractScheduleCreateRequest(now time.Time, days int) ScheduleCreateRequest {
	request := validScheduleCreateRequest()
	request.ContractDigest = "sha256:" + strings.Repeat("c", 64)
	request.ContractExpiresAt = now.AddDate(0, 0, days).Format(scheduleContractTime)
	request.ContractsEnabled = true
	return request
}

func TestIssueScheduleCreateDecisionBindsContractDigestAndExpiry(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	now := time.Date(2026, time.October, 24, 10, 0, 0, 0, time.UTC)

	decision, err := IssueScheduleCreateDecision(evidence, contractScheduleCreateRequest(now, 30), now)
	if err != nil {
		t.Fatalf("a 30 day contract was refused: %v", err)
	}
	if decision.ActionSchemaHash != scheduleCreateContractSchema {
		t.Fatalf("a contract decision must sign the v2 schema, got %s", decision.ActionSchemaHash)
	}

	plain, err := IssueScheduleCreateDecision(evidence, validScheduleCreateRequest(), now)
	if err != nil {
		t.Fatal(err)
	}
	if plain.ActionSchemaHash != scheduleCreateSchema {
		t.Fatalf("a contract-free decision must keep the v1 schema, got %s", plain.ActionSchemaHash)
	}
	shifted := contractScheduleCreateRequest(now, 60)
	second, err := IssueScheduleCreateDecision(evidence, shifted, now)
	if err != nil {
		t.Fatal(err)
	}
	if decision.PayloadDigest == second.PayloadDigest {
		t.Fatal("the payload digest must bind the contract expiry")
	}
}

func TestIssueScheduleCreateDecisionRefusesContractWhileDisabled(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	now := time.Date(2026, time.October, 24, 10, 0, 0, 0, time.UTC)
	request := contractScheduleCreateRequest(now, 30)
	request.ContractsEnabled = false
	if _, err := IssueScheduleCreateDecision(evidence, request, now); err == nil {
		t.Fatal("a contract was authorised with SCHEDULE_CONTRACT_ENABLED off")
	}
}

func TestIssueScheduleCreateDecisionRefusesExpiryOutsideTheWindow(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	now := time.Date(2026, time.October, 24, 10, 0, 0, 0, time.UTC)
	for name, expiry := range map[string]string{
		"beyond the maximum": contractScheduleCreateRequest(now, 400).ContractExpiresAt,
		"already past":       now.Add(-time.Hour).Format(scheduleContractTime),
		"offset form":        now.AddDate(0, 0, 30).Format("2006-01-02T15:04:05+00:00"),
	} {
		t.Run(name, func(t *testing.T) {
			request := contractScheduleCreateRequest(now, 30)
			request.ContractExpiresAt = expiry
			if _, err := IssueScheduleCreateDecision(evidence, request, now); err == nil {
				t.Fatalf("expiry %q was authorised", expiry)
			}
		})
	}
}

func TestIssueScheduleCreateDecisionRefusesContractForZeroDataRetention(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	evidence.Privacy.ZeroDataRetention = true
	now := time.Date(2026, time.October, 24, 10, 0, 0, 0, time.UTC)
	if _, err := IssueScheduleCreateDecision(evidence, contractScheduleCreateRequest(now, 30), now); err == nil {
		t.Fatal("a ZDR Space authorised a contract-bearing schedule")
	}
}

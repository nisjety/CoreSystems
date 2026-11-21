package spaces

import (
	"strings"
	"testing"
	"time"
)

// renewalScheduleCreateRequest is a contract-bearing request that renews the schedule it names.
func renewalScheduleCreateRequest(now time.Time, days int) ScheduleCreateRequest {
	request := contractScheduleCreateRequest(now, days)
	request.RenewalOf = request.ScheduleID
	return request
}

func TestIssueScheduleRenewalDecisionSignsTheRenewalSchemaAndBindsRenewalOf(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	now := time.Date(2026, time.October, 24, 10, 0, 0, 0, time.UTC)

	renewal, err := IssueScheduleCreateDecision(evidence, renewalScheduleCreateRequest(now, 30), now)
	if err != nil {
		t.Fatalf("a renewal of a 30 day contract was refused: %v", err)
	}
	if renewal.ActionSchemaHash != scheduleRenewalSchema {
		t.Fatalf("a renewal must sign the renewal schema, got %s", renewal.ActionSchemaHash)
	}
	if renewal.ActionID != scheduleCreateAction {
		t.Fatalf("a renewal keeps the create action id so entitlements and permissions stay one set, got %s", renewal.ActionID)
	}

	// The same schedule, template and contract as a create must not produce the same payload.
	create, err := IssueScheduleCreateDecision(evidence, contractScheduleCreateRequest(now, 30), now)
	if err != nil {
		t.Fatal(err)
	}
	if create.ActionSchemaHash == renewal.ActionSchemaHash {
		t.Fatal("a create decision and a renewal decision share a schema, so one could be replayed as the other")
	}
	if create.PayloadDigest == renewal.PayloadDigest {
		t.Fatal("the payload digest must bind renewal_of, or a create decision could be replayed as a renewal")
	}
}

func TestIssueScheduleRenewalDecisionPayloadBindsTheRenewedSchedule(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	now := time.Date(2026, time.October, 24, 10, 0, 0, 0, time.UTC)

	first := renewalScheduleCreateRequest(now, 30)
	second := renewalScheduleCreateRequest(now, 30)
	second.ScheduleID = "sched-other"
	second.RenewalOf = "sched-other"
	a, err := IssueScheduleCreateDecision(evidence, first, now)
	if err != nil {
		t.Fatal(err)
	}
	b, err := IssueScheduleCreateDecision(evidence, second, now)
	if err != nil {
		t.Fatal(err)
	}
	if a.PayloadDigest == b.PayloadDigest {
		t.Fatal("a renewal decision for one schedule must not bind another")
	}
}

func TestIssueScheduleRenewalDecisionRefusesAnUnboundOrMismatchedRequest(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	now := time.Date(2026, time.October, 24, 10, 0, 0, 0, time.UTC)

	cases := map[string]func(*ScheduleCreateRequest){
		"renewal of another schedule than it signs for": func(r *ScheduleCreateRequest) { r.RenewalOf = "sched-other" },
		"renewal without a contract digest": func(r *ScheduleCreateRequest) {
			r.ContractDigest = ""
			r.ContractExpiresAt = ""
		},
		"contracts off": func(r *ScheduleCreateRequest) { r.ContractsEnabled = false },
		"expiry beyond the maximum": func(r *ScheduleCreateRequest) {
			r.ContractExpiresAt = now.AddDate(0, 0, 400).Format(scheduleContractTime)
		},
		"expiry already past": func(r *ScheduleCreateRequest) {
			r.ContractExpiresAt = now.Add(-time.Hour).Format(scheduleContractTime)
		},
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			request := renewalScheduleCreateRequest(now, 30)
			mutate(&request)
			if _, err := IssueScheduleCreateDecision(evidence, request, now); err == nil {
				t.Fatalf("%s was authorised", name)
			}
		})
	}
}

func TestIssueScheduleRenewalDecisionRefusesZeroDataRetention(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.ScheduleFireEntitled = true
	evidence.Privacy.ZeroDataRetention = true
	now := time.Date(2026, time.October, 24, 10, 0, 0, 0, time.UTC)
	if _, err := IssueScheduleCreateDecision(evidence, renewalScheduleCreateRequest(now, 30), now); err == nil {
		t.Fatal("a ZDR Space authorised a schedule renewal")
	}
}

// The renewal payload digest is pinned here and in capability-core
// (internal/cron/schedule_renewal_test.go). The two planes hash the same fields in the same order,
// so change this value in both places or in neither.
func TestScheduleRenewalPayloadDigestMatchesCapabilityCoreVector(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	request := ScheduleCreateRequest{
		ScheduleID: "sched-1", TemplateDigest: "sha256:" + strings.Repeat("b", 64),
		IdempotencyKey: "space-cron-renew:sched-1:1", ContractDigest: "sha256:" + strings.Repeat("c", 64),
		ContractExpiresAt: "2026-11-23T10:00:00Z", RenewalOf: "sched-1",
	}
	// The renewal vector uses the same revisions and references as capability-core's copy.
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	got := scheduleCreatePayloadDigest(evidence, request)
	const want = "sha256:3db5664dde4b33a12c58d5489d7ada4be80171736154ec26acd6b60efda7567f"
	if got != want {
		t.Fatalf("renewal payload digest = %s, want %s", got, want)
	}
}

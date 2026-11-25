package http

import (
	"net/http"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// F-f10-27: the scheduled-step decision binds the stored-consent contract only from
// the prepared Session Core authority. The request has no contract field, and a
// binding a body claims is never read.

func scheduledStepDigest(fill byte) string {
	return "sha256:" + strings.Repeat(string(fill), 64)
}

// requestedScheduledStep is the intent the Orchestrator asks about for one step.
func requestedScheduledStep() spaces.ScheduledStepIntent {
	return spaces.ScheduledStepIntent{
		OrgID: "org-1", SpaceRef: "space-1", RunID: "run-1", ThreadID: "thread-1",
		ScheduleID: "schedule-1", FireKey: "fire-1", TemplateDigest: scheduledStepDigest('a'),
		StepID: "run-1:step:0", StepIndex: 0, PolicyDigest: scheduledStepDigest('b'),
		IdempotencyKey: "fire-1:step:0",
	}
}

// preparedScheduledStep is what Session Core prepared for that same step.
func preparedScheduledStep(intent spaces.ScheduledStepIntent) spaces.ScheduledStepAuthority {
	return spaces.ScheduledStepAuthority{
		RunID: intent.RunID, ThreadID: intent.ThreadID, OrgID: intent.OrgID, SubjectID: "user-1",
		SpaceRef: intent.SpaceRef, ScheduleID: intent.ScheduleID, FireKey: intent.FireKey,
		TemplateDigest: intent.TemplateDigest, PolicyDigest: intent.PolicyDigest,
		StepID: intent.StepID, StepIndex: intent.StepIndex, IdempotencyKey: intent.IdempotencyKey,
		RunStatus: "running",
	}
}

func TestScheduledStepRequestHasNoContractField(t *testing.T) {
	for _, typ := range []reflect.Type{reflect.TypeOf(scheduledStepDecisionRequest{}), reflect.TypeOf(scheduledStepIntent{})} {
		for i := 0; i < typ.NumField(); i++ {
			field := typ.Field(i)
			if strings.Contains(strings.ToLower(field.Name), "contract") || strings.Contains(field.Tag.Get("json"), "contract") {
				t.Fatalf("%s carries a contract field %q: the binding must come from the prepared authority only", typ.Name(), field.Name)
			}
		}
	}
}

func TestScheduledStepBindingTakesTheContractOnlyFromPrepared(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	expiresAt := now.Add(time.Hour)
	intent := requestedScheduledStep()
	prepared := preparedScheduledStep(intent)
	prepared.ContractDigest = scheduledStepDigest('c')
	prepared.ContractExpiresAt = expiresAt

	// An intent that carries a different digest and expiry, as a claim would, is
	// overwritten by the prepared contract.
	claimed := intent
	claimed.ContractDigest = scheduledStepDigest('d')
	claimed.ContractExpiresAt = now.Add(3 * time.Hour)

	bound, status, message := scheduledStepBoundIntent(claimed, "", prepared, now)
	if status != 0 {
		t.Fatalf("contract-bound step refused: %d %s", status, message)
	}
	if bound.ContractDigest != prepared.ContractDigest || !bound.ContractExpiresAt.Equal(expiresAt) {
		t.Fatalf("bound contract = %q at %s, want the prepared %q at %s",
			bound.ContractDigest, bound.ContractExpiresAt, prepared.ContractDigest, expiresAt)
	}
	if bound.SubjectID != prepared.SubjectID {
		t.Fatalf("bound subject = %q, want the prepared %q", bound.SubjectID, prepared.SubjectID)
	}
}

func TestScheduledStepBindingRefusesAnExpiredContractWith403(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	intent := requestedScheduledStep()
	prepared := preparedScheduledStep(intent)
	prepared.ContractDigest = scheduledStepDigest('c')
	prepared.ContractExpiresAt = now.Add(-time.Minute)

	_, status, message := scheduledStepBoundIntent(intent, "", prepared, now)
	if status != http.StatusForbidden || message != "scheduled_contract_expired" {
		t.Fatalf("expired contract: status=%d message=%q, want 403 scheduled_contract_expired", status, message)
	}
}

func TestScheduledStepBindingKeepsALegacyScheduleOnTheV1Binding(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	intent := requestedScheduledStep()
	prepared := preparedScheduledStep(intent)

	bound, status, message := scheduledStepBoundIntent(intent, "", prepared, now)
	if status != 0 {
		t.Fatalf("legacy step refused: %d %s", status, message)
	}
	if bound.ContractDigest != "" || !bound.ContractExpiresAt.IsZero() {
		t.Fatalf("legacy step gained a contract binding: %#v", bound)
	}
}

func TestScheduledStepBindingStillRefusesAMismatchedAuthority(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	intent := requestedScheduledStep()
	mismatches := map[string]func(*spaces.ScheduledStepAuthority){
		"another run":          func(a *spaces.ScheduledStepAuthority) { a.RunID = "run-2" },
		"another step index":   func(a *spaces.ScheduledStepAuthority) { a.StepIndex = 1 },
		"another policy":       func(a *spaces.ScheduledStepAuthority) { a.PolicyDigest = scheduledStepDigest('e') },
		"another organization": func(a *spaces.ScheduledStepAuthority) { a.OrgID = "org-2" },
	}
	for name, mutate := range mismatches {
		prepared := preparedScheduledStep(intent)
		mutate(&prepared)
		if _, status, _ := scheduledStepBoundIntent(intent, "", prepared, now); status != http.StatusForbidden {
			t.Fatalf("%s: status=%d, want 403", name, status)
		}
	}

	prepared := preparedScheduledStep(intent)
	if _, status, _ := scheduledStepBoundIntent(intent, "user-2", prepared, now); status != http.StatusForbidden {
		t.Fatalf("a requested subject other than the prepared one: status=%d, want 403", status)
	}
	noSubject := preparedScheduledStep(intent)
	noSubject.SubjectID = ""
	if _, status, _ := scheduledStepBoundIntent(intent, "", noSubject, now); status != http.StatusForbidden {
		t.Fatalf("a prepared step without a subject: status=%d, want 403", status)
	}
}

// A contract the Orchestrator body claims never reaches the binding. The body decodes
// through the real request path into a struct with no contract field, and the intent it
// maps to carries none, so the binding is the prepared contract or none at all.
func TestScheduledStepRequestBodyContractClaimIsIgnored(t *testing.T) {
	now := time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)
	body := `{"intent":{"org_id":"org-1","space_ref":"space-1","subject_id":"user-1","run_id":"run-1",` +
		`"thread_id":"thread-1","schedule_id":"schedule-1","fire_key":"fire-1",` +
		`"template_digest":"` + scheduledStepDigest('a') + `","step_id":"run-1:step:0","step_index":0,` +
		`"policy_digest":"` + scheduledStepDigest('b') + `","idempotency_key":"fire-1:step:0",` +
		`"contract_digest":"` + scheduledStepDigest('d') + `","contract_expires_at":"2026-08-16T05:00:00Z"}}`
	c, w := newGinJSONCtx(body)
	var request scheduledStepDecisionRequest
	if !bindSpaceDecisionBody(c, &request, "valid scheduled step intent is required") {
		t.Fatalf("a body with a claimed contract was refused: status=%d", w.Code)
	}
	intent := scheduledStepIntentFromRequest(request.Intent)

	contractBound := preparedScheduledStep(intent)
	contractBound.ContractDigest = scheduledStepDigest('c')
	contractBound.ContractExpiresAt = now.Add(time.Hour)
	bound, status, message := scheduledStepBoundIntent(intent, request.Intent.SubjectID, contractBound, now)
	if status != 0 {
		t.Fatalf("a contract-bound step was refused: %d %s", status, message)
	}
	if bound.ContractDigest != contractBound.ContractDigest || !bound.ContractExpiresAt.Equal(contractBound.ContractExpiresAt) {
		t.Fatalf("the body's claimed contract reached the binding: %q at %s, want the prepared %q at %s",
			bound.ContractDigest, bound.ContractExpiresAt, contractBound.ContractDigest, contractBound.ContractExpiresAt)
	}

	legacy := preparedScheduledStep(intent)
	bound, status, message = scheduledStepBoundIntent(intent, request.Intent.SubjectID, legacy, now)
	if status != 0 {
		t.Fatalf("a legacy step was refused: %d %s", status, message)
	}
	if bound.ContractDigest != "" || !bound.ContractExpiresAt.IsZero() {
		t.Fatalf("a legacy step bound the body's claimed contract: %#v", bound)
	}
}

package clients

import (
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// F-f10-27: the scheduled-step authority carries the Control-signed contract digest and
// expiry from Session Core (runs.proto tags 15 and 16), and refuses a partial pair.

func preparedScheduledStepResponse() *mpv1.ResolveScheduledStepAuthorityResponse {
	return &mpv1.ResolveScheduledStepAuthorityResponse{
		Resolved: true, RunId: "run-1", ThreadId: "thread-1", OrgId: "org-1", SubjectId: "user-1", SpaceId: "space-1",
		ScheduleId: "schedule-1", FireKey: "fire-1",
		TemplateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		PolicyDigest:   "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		StepId:         "run-1:step:0", StepIndex: 0, IdempotencyKey: "fire-1:step:0", RunStatus: "running",
	}
}

func TestScheduledStepAuthorityMappingCarriesTheContractBinding(t *testing.T) {
	legacy, err := scheduledStepAuthorityFromResponse(preparedScheduledStepResponse())
	if err != nil {
		t.Fatalf("legacy scheduled-step authority mapping: %v", err)
	}
	if legacy.ContractDigest != "" || !legacy.ContractExpiresAt.IsZero() {
		t.Fatalf("a legacy step must carry no contract binding: %#v", legacy)
	}

	const digest = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
	bound := preparedScheduledStepResponse()
	bound.ContractDigest = digest
	bound.ContractExpiresAt = "2026-08-16T01:00:00Z"
	authority, err := scheduledStepAuthorityFromResponse(bound)
	if err != nil {
		t.Fatalf("contract-bound scheduled-step authority mapping: %v", err)
	}
	if authority.ContractDigest != digest || !authority.ContractExpiresAt.Equal(time.Date(2026, 8, 16, 1, 0, 0, 0, time.UTC)) {
		t.Fatalf("contract binding was not carried: %#v", authority)
	}
}

func TestScheduledStepAuthorityMappingRefusesAPartialContractBinding(t *testing.T) {
	const digest = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
	for name, mutate := range map[string]func(*mpv1.ResolveScheduledStepAuthorityResponse){
		"a digest without an expiry": func(r *mpv1.ResolveScheduledStepAuthorityResponse) {
			r.ContractDigest = digest
		},
		"an expiry without a digest": func(r *mpv1.ResolveScheduledStepAuthorityResponse) {
			r.ContractExpiresAt = "2026-08-16T01:00:00Z"
		},
		"an expiry that is not RFC 3339": func(r *mpv1.ResolveScheduledStepAuthorityResponse) {
			r.ContractDigest = digest
			r.ContractExpiresAt = "16/08/2026 01:00"
		},
	} {
		response := preparedScheduledStepResponse()
		mutate(response)
		if _, err := scheduledStepAuthorityFromResponse(response); err == nil {
			t.Fatalf("%s: the scheduled-step authority accepted it", name)
		}
	}
}

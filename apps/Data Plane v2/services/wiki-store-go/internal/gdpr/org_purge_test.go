package gdpr

import (
	"context"
	"encoding/json"
	"errors"
	"testing"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/repo"
)

// fakePurger is an in-test OrgPurger: it records every orgID it was asked
// to purge and returns a canned PurgeResult, so HandleOrgErasure is testable
// with no real Postgres or NATS wiring — mirroring documents-api-go's
// fakeRepository / quarry-control's fakePurger pattern.
type fakePurger struct {
	calls  []string
	result repo.PurgeResult
	err    error
}

func (f *fakePurger) HardPurgeByOrg(_ context.Context, orgID string) (repo.PurgeResult, error) {
	f.calls = append(f.calls, orgID)
	return f.result, f.err
}

func orgErasurePayload(t *testing.T, orgID, requestedBy string) []byte {
	t.Helper()
	// Mirrors org-core's PublishGDPRErasureFanout payload exactly:
	// apps/Control Plane/org-core/internal/org/gdpr_erasure_fanout.go.
	body, err := json.Marshal(map[string]any{
		"subject_type": "organization",
		"subject_id":   orgID,
		"org_id":       orgID,
		"requested_by": requestedBy,
		"ts":           "2026-07-20T12:00:00.000000000Z",
	})
	if err != nil {
		t.Fatalf("marshal fixture payload: %v", err)
	}
	return body
}

func TestHandleOrgErasure_OrganizationSubjectPurgesTheOrg(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: repo.PurgeResult{WikiPagesDeleted: 3, OperatingMapsDeleted: 1}}
	payload := orgErasurePayload(t, "org_a", "system:org-core-purge-cron")

	result, err := HandleOrgErasure(context.Background(), purger, payload)
	if err != nil {
		t.Fatalf("HandleOrgErasure: %v", err)
	}
	if len(purger.calls) != 1 || purger.calls[0] != "org_a" {
		t.Fatalf("purger.calls=%v want=[org_a]", purger.calls)
	}
	if result != purger.result {
		t.Errorf("result=%+v want=%+v", result, purger.result)
	}
}

// TestHandleOrgErasure_UserSubjectIsNoOp proves a "user"/"user_anonymize"
// erasure event — the SAME subject, published by user-core for a different
// purpose — is ignored rather than mishandled as an org purge. wiki-store-go
// has no per-user data model; only "organization" is this consumer's
// concern (see the CRITICAL SAFETY RULE this test enforces: subject_type
// must gate strictly, never a routing-only org_id on a per-user event).
func TestHandleOrgErasure_UserSubjectIsNoOp(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: repo.PurgeResult{WikiPagesDeleted: 99}}
	payload, err := json.Marshal(map[string]any{
		"event_id":     "evt_123",
		"subject_type": "user",
		"subject_id":   "user_a",
		"org_id":       "org_a",
		"requested_by": "user_a",
		"mode":         "hard",
		"ts":           "2026-07-20T12:00:00.000000000Z",
	})
	if err != nil {
		t.Fatalf("marshal fixture: %v", err)
	}

	result, err := HandleOrgErasure(context.Background(), purger, payload)
	if err != nil {
		t.Fatalf("HandleOrgErasure: %v", err)
	}
	if len(purger.calls) != 0 {
		t.Fatalf("purger.calls=%v want=[] — a user-subject event must never trigger an org-wide purge", purger.calls)
	}
	if result.Total() != 0 {
		t.Errorf("result.Total()=%d want=0", result.Total())
	}
}

// TestHandleOrgErasure_UserAnonymizeSubjectIsNoOp is the same safety
// contract for the third known subject_type this shared subject carries.
func TestHandleOrgErasure_UserAnonymizeSubjectIsNoOp(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: repo.PurgeResult{WikiPagesDeleted: 99}}
	payload, err := json.Marshal(map[string]any{
		"subject_type": "user_anonymize",
		"subject_id":   "user_a",
		"org_id":       "org_a",
		"requested_by": "user_a",
		"ts":           "2026-07-20T12:00:00.000000000Z",
	})
	if err != nil {
		t.Fatalf("marshal fixture: %v", err)
	}

	result, err := HandleOrgErasure(context.Background(), purger, payload)
	if err != nil {
		t.Fatalf("HandleOrgErasure: %v", err)
	}
	if len(purger.calls) != 0 {
		t.Fatalf("purger.calls=%v want=[] — user_anonymize must never trigger an org-wide purge", purger.calls)
	}
	if result.Total() != 0 {
		t.Errorf("result.Total()=%d want=0", result.Total())
	}
}

func TestHandleOrgErasure_MissingOrgIDIsPoison(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	payload := orgErasurePayload(t, "", "actor")

	_, err := HandleOrgErasure(context.Background(), purger, payload)
	if err == nil {
		t.Fatal("HandleOrgErasure(blank org_id and subject_id) = nil error, want an error")
	}
	var poison *poisonEventError
	if !errors.As(err, &poison) {
		t.Errorf("err=%v (%T) want a *poisonEventError", err, err)
	}
	if len(purger.calls) != 0 {
		t.Fatalf("purger.calls=%v want=[] — a blank org_id must never be forwarded to HardPurgeByOrg", purger.calls)
	}
}

func TestHandleOrgErasure_MalformedPayloadReturnsError(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	_, err := HandleOrgErasure(context.Background(), purger, []byte("not json"))
	if err == nil {
		t.Fatal("HandleOrgErasure(malformed payload) = nil error, want an error")
	}
	// Must be classified as poison (subscriber.go acks immediately on this,
	// never retries), not treated the same as a transient store failure.
	var poison *poisonEventError
	if !errors.As(err, &poison) {
		t.Errorf("err=%v (%T) want a *poisonEventError so the subscriber acks instead of retrying", err, err)
	}
	if len(purger.calls) != 0 {
		t.Fatalf("purger.calls=%v want=[] on a decode failure", purger.calls)
	}
}

// TestHandleOrgErasure_OrgIDFallsBackToSubjectID mirrors
// conversation-core-go's and quarry-control's org-erasure consumers'
// defensive fallback for this exact event: org-core's real publisher always
// sets both org_id and subject_id to the same value, but a payload with
// org_id blank and only subject_id populated must still resolve to a purge,
// not a silent no-op.
func TestHandleOrgErasure_OrgIDFallsBackToSubjectID(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: repo.PurgeResult{WikiPagesDeleted: 1}}
	payload, err := json.Marshal(map[string]any{
		"subject_type": "organization",
		"subject_id":   "org_a",
		"org_id":       "",
		"requested_by": "system:org-core-purge-cron",
		"ts":           "2026-07-20T12:00:00.000000000Z",
	})
	if err != nil {
		t.Fatalf("marshal fixture: %v", err)
	}

	if _, err := HandleOrgErasure(context.Background(), purger, payload); err != nil {
		t.Fatalf("HandleOrgErasure: %v", err)
	}
	if len(purger.calls) != 1 || purger.calls[0] != "org_a" {
		t.Fatalf("purger.calls=%v want=[org_a]", purger.calls)
	}
}

// TestHandleOrgErasure_RepositoryFailurePropagatesAndIsNotPoison proves a
// transient repository failure (e.g. Postgres unreachable) surfaces as a
// plain error — NOT a *poisonEventError — so subscriber.go's handleDelivery
// naks it for redelivery instead of permanently dropping a legitimate
// organization erasure.
func TestHandleOrgErasure_RepositoryFailurePropagatesAndIsNotPoison(t *testing.T) {
	t.Parallel()
	transientErr := errors.New("connection refused")
	purger := &fakePurger{err: transientErr}
	payload := orgErasurePayload(t, "org_a", "system:org-core-purge-cron")

	_, err := HandleOrgErasure(context.Background(), purger, payload)
	if err == nil {
		t.Fatal("HandleOrgErasure = nil error, want the repository's transient failure")
	}
	var poison *poisonEventError
	if errors.As(err, &poison) {
		t.Fatalf("transient repository failure misclassified as poison: %v", err)
	}
	if len(purger.calls) != 1 || purger.calls[0] != "org_a" {
		t.Fatalf("purger.calls=%v want=[org_a]", purger.calls)
	}
}

// TestHandleOrgErasure_IsIdempotentAcrossRedelivery mirrors the fixed
// contract's requirement directly: NATS is at-least-once, so the same
// erasure event can be delivered twice. Calling HandleOrgErasure twice must
// purge twice without erroring — the underlying repository makes the SECOND
// call a no-op (see repo.TestHardPurgeByOrgIsOrgScopedAndIdempotent), but
// this proves the gdpr package's own no-op/organization-subject gating
// never turns a legitimate redelivery into an error either.
func TestHandleOrgErasure_IsIdempotentAcrossRedelivery(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: repo.PurgeResult{WikiPagesDeleted: 1}}
	payload := orgErasurePayload(t, "org_a", "system:org-core-purge-cron")

	if _, err := HandleOrgErasure(context.Background(), purger, payload); err != nil {
		t.Fatalf("first HandleOrgErasure: %v", err)
	}
	if _, err := HandleOrgErasure(context.Background(), purger, payload); err != nil {
		t.Fatalf("second HandleOrgErasure (redelivery): %v", err)
	}
	if len(purger.calls) != 2 || purger.calls[0] != "org_a" || purger.calls[1] != "org_a" {
		t.Fatalf("purger.calls=%v want=[org_a org_a]", purger.calls)
	}
}

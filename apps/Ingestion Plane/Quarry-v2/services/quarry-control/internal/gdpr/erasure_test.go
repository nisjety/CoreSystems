package gdpr

import (
	"encoding/json"
	"errors"
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// fakePurger is an in-test OrgPurger: it records every orgID it was asked
// to purge and returns a canned PurgeResult, so HandleErasure is testable
// with no real store or NATS wiring — mirroring documents-api-go's
// fakeRepository pattern for its own HandleErasure test.
type fakePurger struct {
	calls  []string
	result store.PurgeResult
	err    error
}

func (f *fakePurger) PurgeOrg(orgID string) (store.PurgeResult, error) {
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

func TestHandleErasure_OrganizationSubjectPurgesTheOrg(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: store.PurgeResult{JobsDeleted: 3, SourcesDeleted: 1}}
	payload := orgErasurePayload(t, "org_a", "system:org-core-purge-cron")

	result, err := HandleErasure(purger, payload)
	if err != nil {
		t.Fatalf("HandleErasure: %v", err)
	}
	if len(purger.calls) != 1 || purger.calls[0] != "org_a" {
		t.Fatalf("purger.calls=%v want=[org_a]", purger.calls)
	}
	if result != purger.result {
		t.Errorf("result=%+v want=%+v", result, purger.result)
	}
}

// TestHandleErasure_UserSubjectIsNoOp proves a "user"/"user_anonymize"
// erasure event — the SAME subject, published by user-core for a different
// purpose — is ignored rather than mishandled as an org purge. Quarry
// control has no per-user data model; only "organization" is its concern.
func TestHandleErasure_UserSubjectIsNoOp(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: store.PurgeResult{JobsDeleted: 99}}
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

	result, err := HandleErasure(purger, payload)
	if err != nil {
		t.Fatalf("HandleErasure: %v", err)
	}
	if len(purger.calls) != 0 {
		t.Fatalf("purger.calls=%v want=[] — a user-subject event must never trigger an org purge", purger.calls)
	}
	if result.Total() != 0 {
		t.Errorf("result.Total()=%d want=0", result.Total())
	}
}

func TestHandleErasure_MissingOrgIDIsNoOp(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	payload := orgErasurePayload(t, "", "actor")

	result, err := HandleErasure(purger, payload)
	if err != nil {
		t.Fatalf("HandleErasure: %v", err)
	}
	if len(purger.calls) != 0 {
		t.Fatalf("purger.calls=%v want=[] — a blank org_id must never be forwarded to PurgeOrg", purger.calls)
	}
	if result.Total() != 0 {
		t.Errorf("result.Total()=%d want=0", result.Total())
	}
}

func TestHandleErasure_MalformedPayloadReturnsError(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	_, err := HandleErasure(purger, []byte("not json"))
	if err == nil {
		t.Fatal("HandleErasure(malformed payload) = nil error, want an error")
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

// TestHandleErasure_OrgIDFallsBackToSubjectID mirrors
// conversation-core-go's org-erasure consumer's defensive fallback for this
// exact event: org-core's real publisher always sets both org_id and
// subject_id to the same value, but a payload with org_id blank and only
// subject_id populated must still resolve to a purge, not a silent no-op.
func TestHandleErasure_OrgIDFallsBackToSubjectID(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: store.PurgeResult{JobsDeleted: 1}}
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

	if _, err := HandleErasure(purger, payload); err != nil {
		t.Fatalf("HandleErasure: %v", err)
	}
	if len(purger.calls) != 1 || purger.calls[0] != "org_a" {
		t.Fatalf("purger.calls=%v want=[org_a]", purger.calls)
	}
}

// TestHandleErasure_IsIdempotentAcrossRedelivery mirrors the fixed
// contract's requirement directly: NATS is at-least-once, so the same
// erasure event can be delivered twice. Calling HandleErasure twice must
// purge twice without erroring — the underlying store makes the SECOND
// call a no-op (see store.TestMemoryPurgeOrg_IsIdempotent), but this proves
// the gdpr package's own no-op/organization-subject gating never turns a
// legitimate redelivery into an error either.
func TestHandleErasure_IsIdempotentAcrossRedelivery(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: store.PurgeResult{JobsDeleted: 1}}
	payload := orgErasurePayload(t, "org_a", "system:org-core-purge-cron")

	if _, err := HandleErasure(purger, payload); err != nil {
		t.Fatalf("first HandleErasure: %v", err)
	}
	if _, err := HandleErasure(purger, payload); err != nil {
		t.Fatalf("second HandleErasure (redelivery): %v", err)
	}
	if len(purger.calls) != 2 || purger.calls[0] != "org_a" || purger.calls[1] != "org_a" {
		t.Fatalf("purger.calls=%v want=[org_a org_a]", purger.calls)
	}
}

// fakeEdge is an in-test EdgePurger: it records every org it was asked to purge
// in quarry-edge and returns a canned result or error (WS17.E3).
type fakeEdge struct {
	calls  []string
	result EdgePurgeResult
	err    error
}

func (f *fakeEdge) PurgeEdgeOrg(orgID string) (EdgePurgeResult, error) {
	f.calls = append(f.calls, orgID)
	return f.result, f.err
}

func TestHandleErasureWithEdge_PurgesStoreThenEdge(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: store.PurgeResult{JobsDeleted: 2}}
	edge := &fakeEdge{result: EdgePurgeResult{Families: []EdgePurgeFamily{{Family: "profiles", RowsDeleted: 3, TombstonesDeleted: 1, CacheKeysDeleted: 4}}}}
	payload := orgErasurePayload(t, "org_a", "system:org-core-purge-cron")

	result, edgeResult, err := HandleErasureWithEdge(purger, edge, payload)
	if err != nil {
		t.Fatalf("HandleErasureWithEdge: %v", err)
	}
	if len(purger.calls) != 1 || purger.calls[0] != "org_a" {
		t.Fatalf("store purger calls=%v want=[org_a]", purger.calls)
	}
	if len(edge.calls) != 1 || edge.calls[0] != "org_a" {
		t.Fatalf("edge calls=%v want=[org_a]", edge.calls)
	}
	if result != purger.result {
		t.Errorf("store result=%+v want=%+v", result, purger.result)
	}
	if edgeResult.Total() != 8 {
		t.Errorf("edge Total()=%d want=8", edgeResult.Total())
	}
}

func TestHandleErasureWithEdge_NilEdgeIsTheStoreOnly(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	payload := orgErasurePayload(t, "org_a", "actor")

	if _, edgeResult, err := HandleErasureWithEdge(purger, nil, payload); err != nil {
		t.Fatalf("HandleErasureWithEdge(nil edge): %v", err)
	} else if edgeResult.Total() != 0 {
		t.Errorf("edge Total()=%d want=0 when the hop is off", edgeResult.Total())
	}
	if len(purger.calls) != 1 {
		t.Fatalf("store calls=%v want one", purger.calls)
	}
}

// An edge failure is an error, not a poison event: the subscriber must NAK it so
// JetStream redelivers (a transient edge outage must not drop the erasure).
func TestHandleErasureWithEdge_EdgeFailureIsRetryableNotPoison(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	edge := &fakeEdge{err: errors.New("gdpr: edge purge call failed")}
	payload := orgErasurePayload(t, "org_a", "actor")

	_, _, err := HandleErasureWithEdge(purger, edge, payload)
	if err == nil {
		t.Fatal("edge failure returned nil error; the erasure would be acked and lost")
	}
	var poison *poisonEventError
	if errors.As(err, &poison) {
		t.Fatalf("edge failure classified as poison (%v); it must be retried", err)
	}
}

func TestHandleErasureWithEdge_UserSubjectNeverReachesTheEdge(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	edge := &fakeEdge{}
	payload, err := json.Marshal(map[string]any{
		"subject_type": "user",
		"subject_id":   "user_a",
		"org_id":       "org_a",
	})
	if err != nil {
		t.Fatalf("marshal fixture: %v", err)
	}
	if _, _, err := HandleErasureWithEdge(purger, edge, payload); err != nil {
		t.Fatalf("HandleErasureWithEdge: %v", err)
	}
	if len(edge.calls) != 0 {
		t.Fatalf("edge calls=%v want none for a user subject", edge.calls)
	}
}

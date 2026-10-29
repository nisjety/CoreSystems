package gdpr

import (
	"context"
	"errors"
	"sync"
	"testing"
)

// fakeOrgPurger records HardPurgeByOrg calls and lets a test inject an error
// so HandleOrgErasure's ack-vs-retry classification can be asserted without a
// real database.
type fakeOrgPurger struct {
	mu     sync.Mutex
	orgIDs []string
	err    error
}

func (f *fakeOrgPurger) HardPurgeByOrg(_ context.Context, orgID string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.err != nil {
		return f.err
	}
	f.orgIDs = append(f.orgIDs, orgID)
	return nil
}

func (f *fakeOrgPurger) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.orgIDs)
}

func TestHandleOrgErasure_OrganizationEvent_PurgesThatOrg(t *testing.T) {
	purger := &fakeOrgPurger{}
	payload := []byte(`{"subject_type":"organization","subject_id":"org-1","org_id":"org-1","requested_by":"user-1","ts":"2026-07-21T00:00:00Z"}`)

	if err := HandleOrgErasure(context.Background(), purger, payload); err != nil {
		t.Fatalf("HandleOrgErasure returned error: %v", err)
	}
	if purger.count() != 1 || purger.orgIDs[0] != "org-1" {
		t.Fatalf("purge calls = %v, want exactly one for org-1", purger.orgIDs)
	}
}

func TestHandleOrgErasure_OrgIDFallsBackToSubjectID(t *testing.T) {
	purger := &fakeOrgPurger{}
	payload := []byte(`{"subject_type":"organization","subject_id":"org-9"}`)

	if err := HandleOrgErasure(context.Background(), purger, payload); err != nil {
		t.Fatalf("HandleOrgErasure returned error: %v", err)
	}
	if purger.count() != 1 || purger.orgIDs[0] != "org-9" {
		t.Fatalf("purge calls = %v, want exactly one for org-9 (from subject_id fallback)", purger.orgIDs)
	}
}

// CRITICAL SAFETY: the same wire subject also carries per-user erasure
// (subject_type "user"/"user_anonymize") with a routing-only org_id. Those
// must never trigger an org-wide purge.
func TestHandleOrgErasure_NonOrganizationSubject_NoOp(t *testing.T) {
	cases := []string{"user", "user_anonymize"}
	for _, subjectType := range cases {
		purger := &fakeOrgPurger{}
		payload := []byte(`{"subject_type":"` + subjectType + `","subject_id":"user-1","org_id":"org-1"}`)
		if err := HandleOrgErasure(context.Background(), purger, payload); err != nil {
			t.Fatalf("subject_type=%s: HandleOrgErasure returned error: %v", subjectType, err)
		}
		if purger.count() != 0 {
			t.Fatalf("subject_type=%s: purge called for a non-organization erasure event: %v", subjectType, purger.orgIDs)
		}
	}
}

// A subject_type case-mismatch ("Organization") must not be treated as a
// match — it must fall through to the same deliberate no-op path as any other
// subject_type this consumer does not own.
func TestHandleOrgErasure_SubjectTypeCaseMismatch_TreatedAsNoOp(t *testing.T) {
	purger := &fakeOrgPurger{}
	payload := []byte(`{"subject_type":"Organization","org_id":"org-1","subject_id":"org-1"}`)

	if err := HandleOrgErasure(context.Background(), purger, payload); err != nil {
		t.Fatalf("HandleOrgErasure returned error: %v", err)
	}
	if purger.count() != 0 {
		t.Fatalf("purge called for a subject_type case-mismatch: %v", purger.orgIDs)
	}
}

func TestHandleOrgErasure_MalformedEvent_ReturnsPoisonError(t *testing.T) {
	cases := map[string][]byte{
		"not json":                      []byte(`not-json`),
		"missing org_id and subject_id": []byte(`{"subject_type":"organization"}`),
		"blank org_id, no fallback":     []byte(`{"subject_type":"organization","org_id":"  "}`),
	}
	for name, payload := range cases {
		purger := &fakeOrgPurger{}
		err := HandleOrgErasure(context.Background(), purger, payload)
		if err == nil {
			t.Fatalf("%s: expected a poison error, got nil", name)
		}
		var poison *poisonOrgEventError
		if !errors.As(err, &poison) {
			t.Errorf("%s: error = %v, want a *poisonOrgEventError", name, err)
		}
		if purger.count() != 0 {
			t.Errorf("%s: purged from a malformed event: %v", name, purger.orgIDs)
		}
	}
}

func TestHandleOrgErasure_TransientPurgeError_IsNotPoison(t *testing.T) {
	purger := &fakeOrgPurger{err: errors.New("db unavailable")}
	payload := []byte(`{"subject_type":"organization","org_id":"org-1","subject_id":"org-1"}`)

	err := HandleOrgErasure(context.Background(), purger, payload)
	if err == nil {
		t.Fatal("expected an error for a transient purge failure")
	}
	var poison *poisonOrgEventError
	if errors.As(err, &poison) {
		t.Fatal("transient purge failure must not be classified as poison (consumer.go would ack instead of retrying)")
	}
}

// NATS is at-least-once delivery: a redelivered erasure event must be
// processed twice without error for the same org (HardPurgeByOrg is
// idempotent by construction; PostgresOrgPurger's own idempotency is verified
// against a real database in org_purge_integration_test.go).
func TestHandleOrgErasure_RedeliveredEvent_ProcessesTwiceWithoutError(t *testing.T) {
	purger := &fakeOrgPurger{}
	payload := []byte(`{"subject_type":"organization","org_id":"org-1","subject_id":"org-1"}`)

	for i := 0; i < 2; i++ {
		if err := HandleOrgErasure(context.Background(), purger, payload); err != nil {
			t.Fatalf("delivery %d returned error: %v", i, err)
		}
	}
	if purger.count() != 2 {
		t.Fatalf("purge calls = %d, want 2 (both deliveries processed)", purger.count())
	}
}

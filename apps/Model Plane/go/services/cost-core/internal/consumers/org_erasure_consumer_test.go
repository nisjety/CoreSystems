package consumers

import (
	"context"
	"errors"
	"sync"
	"testing"
)

// fakeOrgPurger records PurgeOrg calls and lets a test inject an error so the
// consumer's ack-vs-retry policy can be asserted.
type fakeOrgPurger struct {
	mu     sync.Mutex
	orgIDs []string
	err    error
}

func (f *fakeOrgPurger) PurgeOrg(_ context.Context, orgID string) error {
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

func TestOrgErasure_OrganizationEvent_PurgesThatOrg(t *testing.T) {
	purger := &fakeOrgPurger{}
	c := &OrgErasureConsumer{purger: purger}
	ev := orgErasureEvent{SubjectType: "organization", SubjectID: "org-1", OrgID: "org-1", RequestedBy: "user-1"}

	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if purger.count() != 1 || purger.orgIDs[0] != "org-1" {
		t.Fatalf("purge calls = %v, want exactly one for org-1", purger.orgIDs)
	}
}

func TestOrgErasure_OrgIDFallsBackToSubjectID(t *testing.T) {
	purger := &fakeOrgPurger{}
	c := &OrgErasureConsumer{purger: purger}
	ev := orgErasureEvent{SubjectType: "organization", SubjectID: "org-9"}

	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if purger.count() != 1 || purger.orgIDs[0] != "org-9" {
		t.Fatalf("purge calls = %v, want exactly one for org-9 (from subject_id fallback)", purger.orgIDs)
	}
}

// Non-organization subject types (e.g. the Per-User Data Ownership erasure
// fan-out) ride the same wire subject but are outside this consumer's
// authority — it must ack without ever calling the purger. This is the
// critical safety gate: a per-user erasure event must never trigger an
// org-wide purge.
func TestOrgErasure_NonOrganizationSubject_AcksWithoutPurging(t *testing.T) {
	for _, subjectType := range []string{"user", "user_anonymize"} {
		purger := &fakeOrgPurger{}
		c := &OrgErasureConsumer{purger: purger}
		ev := orgErasureEvent{SubjectType: subjectType, SubjectID: "user-1", OrgID: "org-1"}

		if got := c.process(context.Background(), ev); got != outcomeAck {
			t.Fatalf("subject_type=%s: outcome = %v, want outcomeAck for a non-organization erasure", subjectType, got)
		}
		if purger.count() != 0 {
			t.Fatalf("subject_type=%s: purge called for a non-organization erasure event: %v", subjectType, purger.orgIDs)
		}
	}
}

func TestOrgErasure_MalformedEvent_AcksWithoutPurging(t *testing.T) {
	cases := []orgErasureEvent{
		{SubjectType: "organization"},                 // missing org_id and subject_id
		{SubjectType: "organization", OrgID: "  "},    // blank org_id, no subject_id fallback
		{SubjectType: "Organization", OrgID: "org-1"}, // subject_type case-mismatch, not exactly "organization"
	}
	for i, ev := range cases {
		purger := &fakeOrgPurger{}
		c := &OrgErasureConsumer{purger: purger}
		if got := c.process(context.Background(), ev); got != outcomeAck {
			t.Fatalf("case %d outcome = %v, want outcomeAck", i, got)
		}
		if purger.count() != 0 {
			t.Errorf("case %d purged from a malformed event: %v", i, purger.orgIDs)
		}
	}
}

func TestOrgErasure_TransientPurgeError_Retries(t *testing.T) {
	purger := &fakeOrgPurger{err: errors.New("db unavailable")}
	c := &OrgErasureConsumer{purger: purger}
	ev := orgErasureEvent{SubjectType: "organization", OrgID: "org-1"}

	if got := c.process(context.Background(), ev); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry for a transient purge failure", got)
	}
}

// NATS is at-least-once delivery: a redelivered erasure event must be
// processed twice without error for the same org (PurgeOrg is idempotent by
// construction).
func TestOrgErasure_RedeliveredEvent_ProcessesTwiceWithoutError(t *testing.T) {
	purger := &fakeOrgPurger{}
	c := &OrgErasureConsumer{purger: purger}
	ev := orgErasureEvent{SubjectType: "organization", OrgID: "org-1"}

	for i := 0; i < 2; i++ {
		if got := c.process(context.Background(), ev); got != outcomeAck {
			t.Fatalf("delivery %d outcome = %v, want outcomeAck", i, got)
		}
	}
	if purger.count() != 2 {
		t.Fatalf("purge calls = %d, want 2 (both deliveries processed)", purger.count())
	}
}

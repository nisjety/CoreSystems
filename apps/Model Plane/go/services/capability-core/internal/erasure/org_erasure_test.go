package erasure

import (
	"context"
	"errors"
	"sync"
	"testing"
)

// fakePurger records the organizations it is asked to purge and can be told to fail.
type fakePurger struct {
	mu     sync.Mutex
	orgIDs []string
	rows   int64
	err    error
}

func (f *fakePurger) PurgeOrg(_ context.Context, orgID string) (int64, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.err != nil {
		return 0, f.err
	}
	f.orgIDs = append(f.orgIDs, orgID)
	return f.rows, nil
}

func (f *fakePurger) calls() []string {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]string(nil), f.orgIDs...)
}

func TestOrganizationEventPurgesThatOrganization(t *testing.T) {
	purger := &fakePurger{rows: 4}
	c := NewOrgErasureConsumer(nil, purger)
	ev := orgErasureEvent{SubjectType: "organization", SubjectID: "org-1", OrgID: "org-1", RequestedBy: "actor-1"}

	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if calls := purger.calls(); len(calls) != 1 || calls[0] != "org-1" {
		t.Fatalf("purge calls = %v, want exactly one for org-1", calls)
	}
}

func TestOrgIDFallsBackToSubjectID(t *testing.T) {
	purger := &fakePurger{}
	c := NewOrgErasureConsumer(nil, purger)
	ev := orgErasureEvent{SubjectType: "organization", SubjectID: "org-9"}

	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if calls := purger.calls(); len(calls) != 1 || calls[0] != "org-9" {
		t.Fatalf("purge calls = %v, want exactly one for org-9 (from the subject_id fallback)", calls)
	}
}

// A per-user erasure rides the same subject, and its org_id is the person's organization. It must never purge that
// organization: one member's personal-data request is not a request to erase everyone's rows.
func TestPerUserErasureNeverPurgesAnOrganization(t *testing.T) {
	for _, subjectType := range []string{"user", "user_anonymize", ""} {
		purger := &fakePurger{}
		c := NewOrgErasureConsumer(nil, purger)
		ev := orgErasureEvent{SubjectType: subjectType, SubjectID: "user-1", OrgID: "org-1"}

		if got := c.process(context.Background(), ev); got != outcomeAck {
			t.Fatalf("subject_type=%q: outcome = %v, want outcomeAck", subjectType, got)
		}
		if calls := purger.calls(); len(calls) != 0 {
			t.Fatalf("subject_type=%q purged %v; a per-user event must purge no organization", subjectType, calls)
		}
	}
}

// Only the exact subject type (after trimming) erases an organization, as in cost-core's consumer.
func TestSubjectTypeMustBeOrganization(t *testing.T) {
	purger := &fakePurger{}
	c := NewOrgErasureConsumer(nil, purger)
	ev := orgErasureEvent{SubjectType: "Organization", OrgID: "org-1"}

	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if calls := purger.calls(); len(calls) != 0 {
		t.Fatalf("a case-mismatched subject type purged %v", calls)
	}
}

// An event that names no erasable organization is acknowledged and purges nothing. The platform scope is never
// erasable, so the platform's own catalogue rows survive any event.
func TestEventWithoutAnErasableOrganizationPurgesNothing(t *testing.T) {
	cases := []orgErasureEvent{
		{SubjectType: "organization"},
		{SubjectType: "organization", OrgID: "  ", SubjectID: "   "},
		{SubjectType: "organization", OrgID: platformScope},
		{SubjectType: "organization", SubjectID: platformScope},
		{SubjectType: "organization", OrgID: "  global  "},
	}
	for i, ev := range cases {
		purger := &fakePurger{}
		c := NewOrgErasureConsumer(nil, purger)
		if got := c.process(context.Background(), ev); got != outcomeAck {
			t.Fatalf("case %d: outcome = %v, want outcomeAck", i, got)
		}
		if calls := purger.calls(); len(calls) != 0 {
			t.Fatalf("case %d purged %v from an event that names no erasable organization", i, calls)
		}
	}
}

// A transient purge failure asks JetStream to redeliver, so the rows are not left behind.
func TestTransientPurgeErrorIsRetried(t *testing.T) {
	purger := &fakePurger{err: errors.New("database unavailable")}
	c := NewOrgErasureConsumer(nil, purger)
	ev := orgErasureEvent{SubjectType: "organization", OrgID: "org-1"}

	if got := c.process(context.Background(), ev); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry for a transient purge failure", got)
	}
}

// NATS delivers at least once. A redelivered event purges again, which deletes zero rows the second time and fails
// nothing.
func TestRedeliveredEventPurgesAgainWithoutError(t *testing.T) {
	purger := &fakePurger{}
	c := NewOrgErasureConsumer(nil, purger)
	ev := orgErasureEvent{SubjectType: "organization", OrgID: "org-1"}

	for i := 0; i < 2; i++ {
		if got := c.process(context.Background(), ev); got != outcomeAck {
			t.Fatalf("delivery %d: outcome = %v, want outcomeAck", i, got)
		}
	}
	if calls := purger.calls(); len(calls) != 2 {
		t.Fatalf("purge calls = %v, want both deliveries processed", calls)
	}
}

func TestDecodeErasureEventReadsOrgCoreEnvelope(t *testing.T) {
	data := []byte(`{"subject_type":"organization","subject_id":"org-1","org_id":"org-1","requested_by":"actor-1","ts":"2026-11-20T10:00:00Z"}`)
	ev, err := decodeErasureEvent(data)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	if ev.SubjectType != "organization" || ev.OrgID != "org-1" || ev.SubjectID != "org-1" || ev.RequestedBy != "actor-1" {
		t.Fatalf("decoded event = %+v", ev)
	}
	if _, err := decodeErasureEvent([]byte("{not json")); err == nil {
		t.Fatal("an undecodable event must be reported, so the handler can acknowledge it as poison")
	}
}

func TestStartRefusesToBindWithoutAJetStreamContext(t *testing.T) {
	c := NewOrgErasureConsumer(nil, &fakePurger{})
	if err := c.Start(); err == nil {
		t.Fatal("Start with no JetStream context must fail, not bind nothing and report success")
	}
	var none *OrgErasureConsumer
	if err := none.Start(); err == nil {
		t.Fatal("Start on a nil consumer must fail")
	}
	none.Stop()
}

// The purger refuses an empty or platform organization before it touches the database, so a bad event cannot reach
// the platform's rows even with a live pool.
func TestPurgeRefusesAnEmptyOrPlatformOrganizationBeforeTheDatabase(t *testing.T) {
	ctx := context.Background()
	for _, orgID := range []string{"", "   ", platformScope, " global "} {
		deleted, err := NewPgPurger(nil).PurgeOrg(ctx, orgID)
		if err == nil || deleted != 0 {
			t.Fatalf("PurgeOrg(%q) = %d, %v; an empty or platform organization must be refused", orgID, deleted, err)
		}
	}
	if _, err := NewPgPurger(nil).PurgeOrg(ctx, "org-1"); err == nil {
		t.Fatal("a purge without a database must fail, not report zero rows deleted")
	}
}

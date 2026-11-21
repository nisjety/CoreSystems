package consumers

import (
	"bytes"
	"context"
	"errors"
	"log"
	"strings"
	"sync"
	"testing"
)

// fakeOrgPurger records every organization it is asked to erase, and returns the counts and error a test sets.
type fakeOrgPurger struct {
	mu     sync.Mutex
	orgIDs []string
	counts map[string]int64
	err    error
}

func (f *fakeOrgPurger) PurgeOrganization(_ context.Context, orgID string) (map[string]int64, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.orgIDs = append(f.orgIDs, orgID)
	if f.err != nil {
		return nil, f.err
	}
	return f.counts, nil
}

func (f *fakeOrgPurger) calls() []string {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]string(nil), f.orgIDs...)
}

// captureLog routes the standard logger into a buffer for the rest of the test, so a test can read exactly what the
// consumer logged.
func captureLog(t *testing.T) *bytes.Buffer {
	t.Helper()
	var buf bytes.Buffer
	previous := log.Writer()
	log.SetOutput(&buf)
	t.Cleanup(func() { log.SetOutput(previous) })
	return &buf
}

func organizationErasure(orgID string) orgErasureEvent {
	return orgErasureEvent{SubjectType: "organization", SubjectID: orgID, OrgID: orgID, RequestedBy: "user-owner"}
}

func TestOrgErasure_OrganizationEvent_PurgesEveryStoreAndAcks(t *testing.T) {
	rows := &fakeOrgPurger{counts: map[string]int64{"notification_requests": 2}}
	pushes := &fakeOrgPurger{counts: map[string]int64{"notification_web_push_subscriptions": 1}}
	s := NewOrgErasureSubscriber(nil, rows, pushes)

	if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureAck {
		t.Fatalf("outcome = %v, want orgErasureAck", got)
	}
	if calls := rows.calls(); len(calls) != 1 || calls[0] != "org-1" {
		t.Fatalf("first store purge calls = %v, want exactly one for org-1", calls)
	}
	if calls := pushes.calls(); len(calls) != 1 || calls[0] != "org-1" {
		t.Fatalf("second store purge calls = %v, want exactly one for org-1", calls)
	}
}

func TestOrgErasure_OrgIDFallsBackToSubjectID(t *testing.T) {
	purger := &fakeOrgPurger{}
	s := NewOrgErasureSubscriber(nil, purger)

	if got := s.process(context.Background(), orgErasureEvent{SubjectType: "organization", SubjectID: "org-9"}); got != orgErasureAck {
		t.Fatalf("outcome = %v, want orgErasureAck", got)
	}
	if calls := purger.calls(); len(calls) != 1 || calls[0] != "org-9" {
		t.Fatalf("purge calls = %v, want exactly one for org-9 from the subject_id fallback", calls)
	}
}

// A subject_type this subscriber does not act on acks without purging anything. The per-user subject types are erased by
// their own path, which org_erasure_user_test.go pins.
func TestOrgErasure_UnknownSubjectType_AcksWithoutPurging(t *testing.T) {
	for _, subjectType := range []string{"team", "device"} {
		purger := &fakeOrgPurger{}
		s := NewOrgErasureSubscriber(nil, purger)

		if got := s.process(context.Background(), orgErasureEvent{SubjectType: subjectType, SubjectID: "user-1", OrgID: "org-1"}); got != orgErasureAck {
			t.Fatalf("%s outcome = %v, want orgErasureAck", subjectType, got)
		}
		if calls := purger.calls(); len(calls) != 0 {
			t.Fatalf("%s erasure purged organization %v, want no purge", subjectType, calls)
		}
	}
}

func TestOrgErasure_MalformedEvent_AcksWithoutPurging(t *testing.T) {
	cases := []orgErasureEvent{
		{SubjectType: "organization"},                 // no org_id and no subject_id
		{SubjectType: "organization", OrgID: "  "},    // blank org_id, nothing to fall back to
		{SubjectType: "Organization", OrgID: "org-1"}, // subject_type is matched exactly
	}
	for i, event := range cases {
		purger := &fakeOrgPurger{}
		s := NewOrgErasureSubscriber(nil, purger)
		if got := s.process(context.Background(), event); got != orgErasureAck {
			t.Fatalf("case %d outcome = %v, want orgErasureAck", i, got)
		}
		if calls := purger.calls(); len(calls) != 0 {
			t.Fatalf("case %d purged from a malformed event: %v", i, calls)
		}
	}
}

// A payload that does not decode is poison. It is acknowledged, so it cannot redeliver forever, and nothing is purged.
func TestOrgErasure_PoisonPayload_AcksWithoutPurging(t *testing.T) {
	purger := &fakeOrgPurger{}
	s := NewOrgErasureSubscriber(nil, purger)
	logged := captureLog(t)

	for _, data := range [][]byte{[]byte("{not json"), nil, []byte(`{"subject_type":`)} {
		if got := s.processPayload(context.Background(), data); got != orgErasureAck {
			t.Fatalf("poison payload outcome = %v, want orgErasureAck", got)
		}
	}
	if calls := purger.calls(); len(calls) != 0 {
		t.Fatalf("poison payload purged: %v", calls)
	}
	if !strings.Contains(logged.String(), "malformed erasure event") {
		t.Fatalf("poison payload log = %q, want the malformed-event line", logged.String())
	}
}

// One failing store makes the message retry, but every other store still runs, so none is skipped by another's outage.
func TestOrgErasure_PurgeError_RetriesAndStillRunsEveryStore(t *testing.T) {
	failing := &fakeOrgPurger{err: errors.New("database unavailable")}
	healthy := &fakeOrgPurger{counts: map[string]int64{"notification_requests": 1}}
	s := NewOrgErasureSubscriber(nil, failing, healthy)
	captureLog(t)

	if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureRetry {
		t.Fatalf("outcome = %v, want orgErasureRetry for a transient purge failure", got)
	}
	if len(failing.calls()) != 1 || len(healthy.calls()) != 1 {
		t.Fatalf("failing calls = %v, healthy calls = %v, want both stores run once", failing.calls(), healthy.calls())
	}
}

// NATS is at-least-once: a redelivered erasure runs every purge again and acks. Each purge is idempotent, so the second
// run removes nothing and fails nothing.
func TestOrgErasure_RedeliveredEvent_PurgesAgainWithoutError(t *testing.T) {
	purger := &fakeOrgPurger{}
	s := NewOrgErasureSubscriber(nil, purger)
	captureLog(t)

	for i := 0; i < 2; i++ {
		if got := s.process(context.Background(), organizationErasure("org-1")); got != orgErasureAck {
			t.Fatalf("delivery %d outcome = %v, want orgErasureAck", i, got)
		}
	}
	if calls := purger.calls(); len(calls) != 2 {
		t.Fatalf("purge calls = %v, want both deliveries processed", calls)
	}
}

// The success log names the organization and the per-table counts. It does not carry the requester's id.
func TestOrgErasure_LogCarriesTheOrganizationAndCountsOnly(t *testing.T) {
	purger := &fakeOrgPurger{counts: map[string]int64{"notification_requests": 2, "notification_feed_items": 3}}
	s := NewOrgErasureSubscriber(nil, purger)
	logged := captureLog(t)

	event := organizationErasure("org-1")
	event.RequestedBy = "actor-secret-7"
	if got := s.process(context.Background(), event); got != orgErasureAck {
		t.Fatalf("outcome = %v, want orgErasureAck", got)
	}
	line := logged.String()
	if !strings.Contains(line, "org-1") || !strings.Contains(line, "notification_feed_items=3 notification_requests=2") {
		t.Fatalf("log = %q, want the organization and its counts in table order", line)
	}
	if strings.Contains(line, "actor-secret-7") {
		t.Fatalf("log = %q, must not carry the requester id", line)
	}
}

// Without a broker the subscriber is a logged no-op, and it never reaches the bind.
func TestOrgErasure_StartWithoutABrokerIsANoOp(t *testing.T) {
	captureLog(t)
	s := NewOrgErasureSubscriber(nil, &fakeOrgPurger{})

	if err := s.Start(context.Background()); err != nil {
		t.Fatalf("Start() without a broker error = %v, want nil", err)
	}
	s.Stop()
	s.Stop()
}

func TestFormatRemovedCountsIsStableInTableOrder(t *testing.T) {
	got := formatRemovedCounts(map[string]int64{"notification_requests": 2, "notification_feed_items": 0, "a_table": 1})
	if want := "a_table=1 notification_feed_items=0 notification_requests=2"; got != want {
		t.Fatalf("formatRemovedCounts() = %q, want %q", got, want)
	}
	if got := formatRemovedCounts(nil); got != "" {
		t.Fatalf("formatRemovedCounts(nil) = %q, want empty", got)
	}
}

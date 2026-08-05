package users

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

type fakeAuditOutboxStore struct {
	rows        []AuditOutboxRow
	enqueued    []AuditOutboxRow
	completed   []string
	failed      []string
	terminal    bool
	claimError  error
	enqueueErr  error
	completeErr error
	failErr     error
}

func (s *fakeAuditOutboxStore) EnqueueAudit(_ context.Context, row AuditOutboxRow) error {
	if s.enqueueErr != nil {
		return s.enqueueErr
	}
	s.enqueued = append(s.enqueued, row)
	s.rows = append(s.rows, row)
	return nil
}

func (s *fakeAuditOutboxStore) ClaimAudit(_ context.Context) (AuditOutboxRow, bool, error) {
	if s.claimError != nil {
		return AuditOutboxRow{}, false, s.claimError
	}
	if len(s.rows) == 0 {
		return AuditOutboxRow{}, false, nil
	}
	row := s.rows[0]
	s.rows = s.rows[1:]
	row.Attempts++
	return row, true, nil
}

func (s *fakeAuditOutboxStore) CompleteAudit(_ context.Context, eventID string, _ int) error {
	if s.completeErr != nil {
		return s.completeErr
	}
	s.completed = append(s.completed, eventID)
	return nil
}

func (s *fakeAuditOutboxStore) FailAudit(_ context.Context, eventID string, _ int, _ time.Time, _ string, terminal bool) error {
	if s.failErr != nil {
		return s.failErr
	}
	s.failed = append(s.failed, eventID)
	s.terminal = terminal
	return nil
}

type fakeAuditJetStream struct {
	failures  int
	ids       []string
	subjects  []string
	published chan string
}

func (p *fakeAuditJetStream) PublishJetStreamWithMsgID(_ context.Context, subject, eventID string, _ []byte) error {
	p.ids = append(p.ids, eventID)
	p.subjects = append(p.subjects, subject)
	if p.published != nil {
		p.published <- eventID
	}
	if p.failures > 0 {
		p.failures--
		return errors.New("jetstream unavailable")
	}
	return nil
}

func TestAuditOutboxRejectsInvalidAuthorityAndPayload(t *testing.T) {
	outbox := newAuditOutbox(&fakeAuditOutboxStore{}, &fakeAuditJetStream{})
	for name, row := range map[string]AuditOutboxRow{
		"missing id":     {Subject: ErasureAuditSubject, Payload: []byte(`{}`)},
		"long id":        {EventID: strings.Repeat("x", 129), Subject: ErasureAuditSubject, Payload: []byte(`{}`)},
		"forged subject": {EventID: "id", Subject: "verevon.audit.v2.control.auth-core.erasure", Payload: []byte(`{}`)},
		"bad payload":    {EventID: "id", Subject: ErasureAuditSubject, Payload: []byte(`nope`)},
	} {
		t.Run(name, func(t *testing.T) {
			if err := outbox.EnqueueAndDispatch(t.Context(), row); err == nil {
				t.Fatal("expected validation error")
			}
		})
	}
}

func TestAuditOutboxSurfacesPersistenceAndLeaseFailures(t *testing.T) {
	persist := newAuditOutbox(&fakeAuditOutboxStore{enqueueErr: errors.New("db unavailable")}, &fakeAuditJetStream{})
	if err := persist.EnqueueAndDispatch(t.Context(), AuditOutboxRow{EventID: "id", Subject: ErasureAuditSubject, Payload: []byte(`{}`)}); err == nil {
		t.Fatal("expected persistence failure")
	}

	claim := newAuditOutbox(&fakeAuditOutboxStore{claimError: errors.New("claim failed")}, &fakeAuditJetStream{})
	if _, err := claim.DispatchOne(t.Context()); err == nil {
		t.Fatal("expected claim failure")
	}

	complete := newAuditOutbox(&fakeAuditOutboxStore{rows: []AuditOutboxRow{{EventID: "id", Subject: ErasureAuditSubject, Payload: []byte(`{}`)}}, completeErr: errors.New("lease lost")}, &fakeAuditJetStream{})
	if _, err := complete.DispatchOne(t.Context()); err == nil {
		t.Fatal("expected completion failure")
	}

	fail := newAuditOutbox(&fakeAuditOutboxStore{rows: []AuditOutboxRow{{EventID: "id", Subject: ErasureAuditSubject, Payload: []byte(`{}`)}}, failErr: errors.New("lease lost")}, &fakeAuditJetStream{failures: 1})
	if _, err := fail.DispatchOne(t.Context()); err == nil {
		t.Fatal("expected failure-recording error")
	}
}

func TestAuditOutboxWorkerDrainsAndStops(t *testing.T) {
	store := &fakeAuditOutboxStore{rows: []AuditOutboxRow{{EventID: "worker-id", Subject: ErasureAuditSubject, Payload: []byte(`{}`)}}}
	published := make(chan string, 1)
	outbox := newAuditOutbox(store, nil)
	outbox.Start(&fakeAuditJetStream{published: published})
	outbox.Start(&fakeAuditJetStream{}) // idempotent
	t.Cleanup(outbox.Close)

	select {
	case got := <-published:
		if got != "worker-id" {
			t.Fatalf("published %q, want worker-id", got)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("worker did not drain queued event")
	}
	outbox.Close()
	outbox.Close()
}

func TestAuditRetryDelayIsBounded(t *testing.T) {
	if got := auditRetryDelay(0); got != time.Second {
		t.Fatalf("first retry = %s", got)
	}
	if got := auditRetryDelay(100); got != 300*time.Second {
		t.Fatalf("capped retry = %s", got)
	}
}

func TestAuditOutboxPersistsBeforePublishingAndCompletesAfterPubAck(t *testing.T) {
	store := &fakeAuditOutboxStore{}
	publisher := &fakeAuditJetStream{}
	outbox := newAuditOutbox(store, publisher)

	err := outbox.EnqueueAndDispatch(t.Context(), AuditOutboxRow{
		EventID: "gdpr:user-core:stable", Subject: ErasureAuditSubject,
		Payload: []byte(`{"event_id":"gdpr:user-core:stable"}`),
	})
	if err != nil {
		t.Fatalf("enqueue and dispatch: %v", err)
	}
	if len(store.enqueued) != 1 || len(publisher.ids) != 1 || len(store.completed) != 1 {
		t.Fatalf("expected persist -> publish -> complete, got enqueued=%d published=%d completed=%d", len(store.enqueued), len(publisher.ids), len(store.completed))
	}
	if publisher.ids[0] != store.enqueued[0].EventID {
		t.Fatalf("Nats-Msg-Id = %q, want stable outbox id %q", publisher.ids[0], store.enqueued[0].EventID)
	}
}

func TestAuditOutboxRetainsFailedPublicationForRetry(t *testing.T) {
	store := &fakeAuditOutboxStore{}
	publisher := &fakeAuditJetStream{failures: 1}
	outbox := newAuditOutbox(store, publisher)

	err := outbox.EnqueueAndDispatch(t.Context(), AuditOutboxRow{
		EventID: "dsar:user-core:stable", Subject: DSARExportAuditSubject,
		Payload: []byte(`{"event_id":"dsar:user-core:stable"}`),
	})
	if err == nil {
		t.Fatal("expected immediate publication failure")
	}
	if len(store.failed) != 1 || store.terminal {
		t.Fatalf("failed event must remain retryable, failed=%v terminal=%v", store.failed, store.terminal)
	}
	if len(store.completed) != 0 {
		t.Fatalf("failed event must not be completed: %v", store.completed)
	}
}

func TestAuditOutboxTerminatesPoisonEventAfterBoundedAttempts(t *testing.T) {
	store := &fakeAuditOutboxStore{rows: []AuditOutboxRow{{
		EventID: "gdpr:user-core:poison", Subject: ErasureAuditSubject,
		Payload: []byte(`{"bad":true}`), Attempts: maxAuditDispatchAttempts - 1,
	}}}
	publisher := &fakeAuditJetStream{failures: 1}
	outbox := newAuditOutbox(store, publisher)

	_, err := outbox.DispatchOne(t.Context())
	if err == nil {
		t.Fatal("expected publication failure")
	}
	if !store.terminal {
		t.Fatal("poison event must become terminal after bounded attempts")
	}
}

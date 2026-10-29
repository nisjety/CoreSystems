package api

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/controlplane"
	"github.com/triodelab/integration-corev2/internal/store"
)

type fakeIntegrationAuditStore struct {
	rows        []store.AuditEvent
	completed   []string
	failed      []string
	terminal    bool
	claimErr    error
	failErr     error
	completeErr error
	stats       store.AuditOutboxStats
	statsErr    error
	requeued    []string
}

func (s *fakeIntegrationAuditStore) AuditOutboxStats(context.Context) (store.AuditOutboxStats, error) {
	return s.stats, s.statsErr
}

func (s *fakeIntegrationAuditStore) RequeueTerminalAuditEvents(_ context.Context, eventIDs []string) (int, error) {
	s.requeued = append([]string(nil), eventIDs...)
	return len(eventIDs), nil
}

func (s *fakeIntegrationAuditStore) ClaimAuditEvent(_ context.Context) (store.AuditEvent, bool, error) {
	if s.claimErr != nil {
		return store.AuditEvent{}, false, s.claimErr
	}
	if len(s.rows) == 0 {
		return store.AuditEvent{}, false, nil
	}
	event := s.rows[0]
	s.rows = s.rows[1:]
	event.Attempts++
	return event, true, nil
}

func (s *fakeIntegrationAuditStore) CompleteAuditEvent(_ context.Context, eventID string, _ int) error {
	if s.completeErr != nil {
		return s.completeErr
	}
	s.completed = append(s.completed, eventID)
	return nil
}

func (s *fakeIntegrationAuditStore) FailAuditEvent(_ context.Context, eventID string, _ int, _ time.Time, _ string, terminal bool) error {
	if s.failErr != nil {
		return s.failErr
	}
	s.failed = append(s.failed, eventID)
	s.terminal = terminal
	return nil
}

type fakeIntegrationAuditRecorder struct {
	failures  int
	events    []controlplane.AuditEvent
	published chan string
}

func (r *fakeIntegrationAuditRecorder) RecordAudit(_ context.Context, event controlplane.AuditEvent) error {
	r.events = append(r.events, event)
	if r.published != nil {
		r.published <- event.EventID
	}
	if r.failures > 0 {
		r.failures--
		return errors.New("audit-core unavailable")
	}
	return nil
}

func TestIntegrationAuditOutboxCompletesOnlyAfterAuditCoreAcceptsEvent(t *testing.T) {
	stored := store.AuditEvent{
		ID: "audit-stable", OrganizationID: "org-1", UserID: "user-1",
		ConnectionID: "connection-1", EventType: "connection.created",
		ProviderKey: "microsoft", CreatedAt: time.Unix(1_700_000_000, 0).UTC(),
	}
	queue := &fakeIntegrationAuditStore{rows: []store.AuditEvent{stored}}
	recorder := &fakeIntegrationAuditRecorder{}
	outbox := NewAuditOutbox(queue, recorder, nil)

	found, err := outbox.DispatchOne(t.Context())
	if err != nil || !found {
		t.Fatalf("dispatch found=%v err=%v", found, err)
	}
	if len(queue.completed) != 1 || len(queue.failed) != 0 {
		t.Fatalf("completion state completed=%v failed=%v", queue.completed, queue.failed)
	}
	if len(recorder.events) != 1 || recorder.events[0].EventID != stored.ID || recorder.events[0].OrgID != stored.OrganizationID {
		t.Fatalf("forwarded event = %+v", recorder.events)
	}
}

func TestIntegrationAuditOutboxRetainsTransientFailureAndBoundsPoisonRetries(t *testing.T) {
	queue := &fakeIntegrationAuditStore{rows: []store.AuditEvent{{
		ID: "audit-retry", OrganizationID: "org-1", EventType: "sync.failed",
		CreatedAt: time.Now().UTC(), Attempts: maxIntegrationAuditAttempts - 1,
	}}}
	recorder := &fakeIntegrationAuditRecorder{failures: 1}
	outbox := NewAuditOutbox(queue, recorder, nil)

	if _, err := outbox.DispatchOne(t.Context()); err == nil {
		t.Fatal("expected Audit Core failure")
	}
	if len(queue.completed) != 0 || len(queue.failed) != 1 || !queue.terminal {
		t.Fatalf("failed delivery state completed=%v failed=%v terminal=%v", queue.completed, queue.failed, queue.terminal)
	}
}

func TestIntegrationAuditOutboxSurfacesStoreFailures(t *testing.T) {
	claim := NewAuditOutbox(&fakeIntegrationAuditStore{claimErr: errors.New("claim failed")}, &fakeIntegrationAuditRecorder{}, nil)
	if _, err := claim.DispatchOne(t.Context()); err == nil {
		t.Fatal("expected claim failure")
	}

	failure := NewAuditOutbox(&fakeIntegrationAuditStore{
		rows:    []store.AuditEvent{{ID: "audit-1", OrganizationID: "org-1", EventType: "sync.failed", CreatedAt: time.Now().UTC()}},
		failErr: errors.New("lease lost"),
	}, &fakeIntegrationAuditRecorder{failures: 1}, nil)
	if _, err := failure.DispatchOne(t.Context()); err == nil {
		t.Fatal("expected retry persistence failure")
	}

	complete := NewAuditOutbox(&fakeIntegrationAuditStore{
		rows:        []store.AuditEvent{{ID: "audit-2", OrganizationID: "org-1", EventType: "sync.ok", CreatedAt: time.Now().UTC()}},
		completeErr: errors.New("lease lost"),
	}, &fakeIntegrationAuditRecorder{}, nil)
	if _, err := complete.DispatchOne(t.Context()); err == nil {
		t.Fatal("expected completion failure")
	}
}

func TestIntegrationAuditOutboxWorkerDrainsAndStops(t *testing.T) {
	queue := &fakeIntegrationAuditStore{rows: []store.AuditEvent{{
		ID: "audit-worker", OrganizationID: "org-1", EventType: "sync.ok", CreatedAt: time.Now().UTC(),
	}}}
	published := make(chan string, 1)
	outbox := NewAuditOutbox(queue, &fakeIntegrationAuditRecorder{published: published}, nil)
	outbox.Start()
	outbox.Start()
	t.Cleanup(outbox.Close)

	select {
	case got := <-published:
		if got != "audit-worker" {
			t.Fatalf("published %q", got)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("audit worker did not drain pending event")
	}
	outbox.Close()
	outbox.Close()
}

func TestIntegrationAuditOutboxWorkerDefersFailedDeliveryUntilRetry(t *testing.T) {
	queue := &fakeIntegrationAuditStore{rows: []store.AuditEvent{{
		ID: "audit-worker-retry", OrganizationID: "org-1", EventType: "sync.failed", CreatedAt: time.Now().UTC(),
	}}}
	attempted := make(chan string, 1)
	outbox := NewAuditOutbox(queue, &fakeIntegrationAuditRecorder{failures: 1, published: attempted}, nil)
	outbox.Start()

	select {
	case got := <-attempted:
		if got != "audit-worker-retry" {
			t.Fatalf("attempted %q", got)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("audit worker did not attempt pending event")
	}
	outbox.Close()
	if len(queue.completed) != 0 || len(queue.failed) != 1 {
		t.Fatalf("failed worker state completed=%v failed=%v", queue.completed, queue.failed)
	}
}

func TestIntegrationAuditOutboxRejectsMissingDependenciesAndBoundsRetry(t *testing.T) {
	if _, err := NewAuditOutbox(nil, nil, nil).DispatchOne(t.Context()); err == nil {
		t.Fatal("expected unconfigured outbox error")
	}
	if got := integrationAuditRetryDelay(0); got != time.Second {
		t.Fatalf("initial retry = %s", got)
	}
	if got := integrationAuditRetryDelay(100); got != 300*time.Second {
		t.Fatalf("capped retry = %s", got)
	}
	var nilOutbox *AuditOutbox
	nilOutbox.Start()
	nilOutbox.Close()
}

func TestIntegrationAuditOutboxExposesTerminalLagAndBoundedRecovery(t *testing.T) {
	queue := &fakeIntegrationAuditStore{stats: store.AuditOutboxStats{
		Pending: 4, Terminal: 2, OldestPendingAge: 3 * time.Minute, OldestTerminalAge: time.Hour,
	}}
	outbox := NewAuditOutbox(queue, &fakeIntegrationAuditRecorder{}, nil)

	status, err := outbox.Status(t.Context())
	if err != nil {
		t.Fatalf("Status error: %v", err)
	}
	if status.Terminal != 2 || !status.Degraded || status.OldestTerminalAge != time.Hour {
		t.Fatalf("Status = %#v, want degraded terminal lag", status)
	}
	count, err := outbox.RequeueTerminal(t.Context(), []string{"audit-1", "audit-2"})
	if err != nil || count != 2 {
		t.Fatalf("RequeueTerminal = (%d, %v), want (2, nil)", count, err)
	}
	if len(queue.requeued) != 2 {
		t.Fatalf("requeued ids = %#v", queue.requeued)
	}
	if _, err := outbox.RequeueTerminal(t.Context(), nil); err == nil {
		t.Fatal("empty terminal requeue was accepted")
	}
	tooMany := make([]string, maxTerminalAuditRequeue+1)
	if _, err := outbox.RequeueTerminal(t.Context(), tooMany); err == nil {
		t.Fatal("oversized terminal requeue was accepted")
	}
	queue.statsErr = errors.New("stats unavailable")
	if _, err := outbox.Status(t.Context()); err == nil {
		t.Fatal("status storage failure was hidden")
	}
	var missing *AuditOutbox
	if _, err := missing.Status(t.Context()); err == nil {
		t.Fatal("unconfigured status was accepted")
	}
}

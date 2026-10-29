package consumers

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

type ticketOutboxStoreFake struct {
	mu           sync.Mutex
	events       []conversation.TicketOperationOutboxEvent
	acknowledged []string
	released     []string
}

func (s *ticketOutboxStoreFake) ClaimTicketOperationOutbox(_ context.Context, _ string, _ time.Time, _ time.Duration, _ int) ([]conversation.TicketOperationOutboxEvent, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	result := append([]conversation.TicketOperationOutboxEvent(nil), s.events...)
	s.events = nil
	return result, nil
}

func (s *ticketOutboxStoreFake) AcknowledgeTicketOperationOutbox(_ context.Context, eventID, _ string, _ time.Time) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.acknowledged = append(s.acknowledged, eventID)
	return nil
}

func (s *ticketOutboxStoreFake) ReleaseTicketOperationOutbox(_ context.Context, eventID, _ string, _ string, _ time.Time) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.released = append(s.released, eventID)
	return nil
}

type ticketOutboxPublisherFake struct {
	err             error
	errorsBySubject map[string]error
	subjects        []string
	events          []conversation.LifecycleEvent
	payloads        []any
}

func (p *ticketOutboxPublisherFake) Publish(_ context.Context, subject string, value any) error {
	p.subjects = append(p.subjects, subject)
	p.payloads = append(p.payloads, value)
	if event, ok := value.(conversation.LifecycleEvent); ok {
		p.events = append(p.events, event)
	}
	if err := p.errorsBySubject[subject]; err != nil {
		return err
	}
	return p.err
}

func TestTicketOperationOutboxAcknowledgesOnlyAfterPublish(t *testing.T) {
	store := &ticketOutboxStoreFake{events: []conversation.TicketOperationOutboxEvent{{
		ID: "evt_ticket_1", OrgID: "org_1", ConversationID: "conv_1", ActorUserID: "user_1",
		Payload: map[string]any{"operation_id": "ticketop_1", "audit_event_id": "audit_1", "ticket_id": "ticket_1"}, CreatedAt: time.Date(2026, 8, 13, 0, 0, 0, 0, time.UTC),
	}}}
	publisher := &ticketOutboxPublisherFake{}
	dispatcher := NewTicketOperationOutboxDispatcher(store, publisher, "worker_1", time.Hour)
	dispatcher.run(context.Background())
	if len(store.acknowledged) != 1 || store.acknowledged[0] != "evt_ticket_1" || len(store.released) != 0 {
		t.Fatalf("ack/release = %#v/%#v", store.acknowledged, store.released)
	}
	if len(publisher.events) != 1 || publisher.subjects[0] != conversation.SubjectTicketCreated || publisher.events[0].ID != "evt_ticket_1" {
		t.Fatalf("published = %#v subjects=%#v", publisher.events, publisher.subjects)
	}
	if len(publisher.subjects) != 2 || publisher.subjects[1] != "verevon.audit.v2.application.conversation-core.ticket_created" {
		t.Fatalf("subjects = %#v, want domain event then audit observation", publisher.subjects)
	}
	audit, ok := publisher.payloads[1].(conversation.AuditObservation)
	if !ok || audit.ID != "audit_1" || audit.OrgID != "org_1" || audit.UserID != "user_1" ||
		audit.Subject != "conv_1" || audit.ResourceID != "ticket_1" || audit.Details["operation_id"] != "ticketop_1" {
		t.Fatalf("audit observation = %#v", publisher.payloads[1])
	}
}

func TestTicketOperationOutboxReleasesOnPublishFailure(t *testing.T) {
	store := &ticketOutboxStoreFake{events: []conversation.TicketOperationOutboxEvent{{ID: "evt_ticket_1"}}}
	publisher := &ticketOutboxPublisherFake{err: errors.New("broker unavailable")}
	dispatcher := NewTicketOperationOutboxDispatcher(store, publisher, "worker_1", time.Hour)
	dispatcher.run(context.Background())
	if len(store.acknowledged) != 0 || len(store.released) != 1 || store.released[0] != "evt_ticket_1" {
		t.Fatalf("ack/release = %#v/%#v", store.acknowledged, store.released)
	}
}

func TestTicketOperationOutboxDoesNotAcknowledgeWhenAuditPublishFails(t *testing.T) {
	store := &ticketOutboxStoreFake{events: []conversation.TicketOperationOutboxEvent{{
		ID: "evt_ticket_1", OrgID: "org_1", ConversationID: "conv_1", ActorUserID: "user_1",
		Payload:   map[string]any{"operation_id": "ticketop_1", "audit_event_id": "audit_1", "ticket_id": "ticket_1"},
		CreatedAt: time.Date(2026, 8, 13, 0, 0, 0, 0, time.UTC),
	}}}
	publisher := &ticketOutboxPublisherFake{errorsBySubject: map[string]error{
		conversation.SubjectTicketCreatedAudit: errors.New("audit stream unavailable"),
	}}
	dispatcher := NewTicketOperationOutboxDispatcher(store, publisher, "worker_1", time.Hour)
	dispatcher.run(context.Background())

	if len(store.acknowledged) != 0 || len(store.released) != 1 || store.released[0] != "evt_ticket_1" {
		t.Fatalf("ack/release = %#v/%#v, want retryable audit obligation", store.acknowledged, store.released)
	}
	if got := publisher.subjects; len(got) != 2 || got[0] != conversation.SubjectTicketCreated || got[1] != conversation.SubjectTicketCreatedAudit {
		t.Fatalf("publish order = %#v, want domain event then failed audit event", got)
	}
}

package consumers

import (
	"context"
	"fmt"
	"log"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

// TicketOperationOutboxDispatcher publishes only owner-issued ticket-operation
// events. It provides at-least-once delivery: a broker acknowledgement is
// followed by a leased database acknowledgement, so a crash in between safely
// produces a duplicate with the same event ID rather than losing activity.
type TicketOperationOutboxDispatcher struct {
	store      conversation.TicketOperationOutboxStore
	publisher  conversation.EventPublisher
	interval   time.Duration
	lease      time.Duration
	runTimeout time.Duration
	workerID   string
	stop       chan struct{}
	done       chan struct{}
}

func NewTicketOperationOutboxDispatcher(
	store conversation.TicketOperationOutboxStore,
	publisher conversation.EventPublisher,
	workerID string,
	interval time.Duration,
) *TicketOperationOutboxDispatcher {
	return &TicketOperationOutboxDispatcher{
		store: store, publisher: publisher, workerID: workerID,
		interval: interval, lease: 30 * time.Second, runTimeout: 20 * time.Second,
		stop: make(chan struct{}), done: make(chan struct{}),
	}
}

func (d *TicketOperationOutboxDispatcher) Start(ctx context.Context) {
	go func() {
		defer close(d.done)
		d.run(ctx)
		ticker := time.NewTicker(d.interval)
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				d.run(ctx)
			case <-d.stop:
				return
			}
		}
	}()
}

func (d *TicketOperationOutboxDispatcher) Stop() {
	close(d.stop)
	<-d.done
}

func (d *TicketOperationOutboxDispatcher) run(ctx context.Context) {
	if d.store == nil || d.publisher == nil || d.interval <= 0 || d.workerID == "" {
		return
	}
	runCtx, cancel := context.WithTimeout(ctx, d.runTimeout)
	defer cancel()
	now := time.Now().UTC()
	events, err := d.store.ClaimTicketOperationOutbox(runCtx, d.workerID, now, d.lease, 32)
	if err != nil {
		log.Printf("[cc-go/ticket-operation-outbox] claim failed: %v", err)
		return
	}
	for _, event := range events {
		lifecycle := conversation.LifecycleEvent{
			ID: event.ID, Type: "ticket.created", OrgID: event.OrgID,
			ConversationID: event.ConversationID, ActorUserID: event.ActorUserID,
			Data: event.Payload, OccurredAt: event.CreatedAt,
		}
		if err := d.publisher.Publish(runCtx, conversation.SubjectTicketCreated, lifecycle); err != nil {
			d.release(event.ID, err, runCtx)
			continue
		}
		audit, err := ticketOperationAuditObservation(event)
		if err != nil {
			d.release(event.ID, err, runCtx)
			continue
		}
		if err := d.publisher.Publish(runCtx, conversation.SubjectTicketCreatedAudit, audit); err != nil {
			d.release(event.ID, err, runCtx)
			continue
		}
		if err := d.store.AcknowledgeTicketOperationOutbox(runCtx, event.ID, d.workerID, time.Now().UTC()); err != nil {
			// The broker may already have accepted the event. Keep it unacked so a
			// later delivery repeats the same ID rather than claiming a lost event.
			log.Printf("[cc-go/ticket-operation-outbox] acknowledge %s failed: %v", event.ID, err)
		}
	}
}

func (d *TicketOperationOutboxDispatcher) release(eventID string, cause error, ctx context.Context) {
	retryAt := time.Now().UTC().Add(time.Minute)
	if releaseErr := d.store.ReleaseTicketOperationOutbox(ctx, eventID, d.workerID, boundedOutboxError(cause), retryAt); releaseErr != nil {
		log.Printf("[cc-go/ticket-operation-outbox] release %s failed: %v", eventID, releaseErr)
	}
}

func ticketOperationAuditObservation(event conversation.TicketOperationOutboxEvent) (conversation.AuditObservation, error) {
	operationID, _ := event.Payload["operation_id"].(string)
	auditID, _ := event.Payload["audit_event_id"].(string)
	ticketID, _ := event.Payload["ticket_id"].(string)
	if operationID == "" || auditID == "" || ticketID == "" || event.OrgID == "" || event.ConversationID == "" {
		return conversation.AuditObservation{}, fmt.Errorf("ticket operation event is missing audit correlation")
	}
	return conversation.AuditObservation{
		ID: auditID, OccurredAt: event.CreatedAt, OrgID: event.OrgID, UserID: event.ActorUserID,
		Plane: "application", Producer: "conversation-core", Event: "ticket_created",
		Subject: event.ConversationID, ResourceID: ticketID, Outcome: "ok",
		Details: map[string]any{"operation_id": operationID},
	}, nil
}

func boundedOutboxError(err error) string {
	value := "ticket operation outbox publish failed"
	if err != nil {
		value = err.Error()
	}
	if len(value) > 256 {
		value = value[:256]
	}
	return fmt.Sprintf("%s", value)
}

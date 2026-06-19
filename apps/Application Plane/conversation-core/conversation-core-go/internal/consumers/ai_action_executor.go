package consumers

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"strings"
	"time"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

const (
	aiActionExecutorDurable  = "conversation-core-ai-action-executor"
	kindTicketClassification = "ticket.classification"
	executorActor            = "ai-action-executor"
)

// ActionStore is the narrow read/claim surface the executor needs.
// *conversation.PGRepository satisfies it.
type ActionStore interface {
	GetAIAction(ctx context.Context, orgID, id string) (*conversation.AIAction, error)
	GetTicketByConversation(ctx context.Context, orgID, conversationID string) (*conversation.Ticket, error)
	MarkAIActionExecuted(ctx context.Context, orgID, id string) (bool, error)
	UnmarkAIActionExecuted(ctx context.Context, orgID, id string) error
}

// TicketPromoter applies the approved routing. *conversation.Service satisfies
// it, so promotion runs through the normal normalize + publish + automation path.
type TicketPromoter interface {
	UpdateTicket(ctx context.Context, input conversation.UpdateTicketInput) (*conversation.Ticket, error)
}

// Publisher emits the post-execution event. *eventing.Publisher satisfies it.
type Publisher interface {
	Publish(ctx context.Context, subject string, payload any) error
}

// AIActionExecutor completes the HITL loop: when a human approves a
// ticket.classification action, it promotes the suggested ticket and applies
// the AI's suggested routing — the moment "approve" stops being a no-op. It is
// idempotent by AIAction id (an atomic approved→executed claim), so duplicate or
// redelivered ai_action.reviewed events never double-apply.
//
// Plane rule: this executor lives in conversation-core (Application Plane),
// never the Model Plane.
type AIActionExecutor struct {
	consumer  *DurableConsumer
	store     ActionStore
	tickets   TicketPromoter
	publisher Publisher
}

func NewAIActionExecutor(js nats.JetStreamContext, store ActionStore, tickets TicketPromoter, publisher Publisher) *AIActionExecutor {
	return &AIActionExecutor{
		consumer:  NewDurableConsumer(js, "ai-action-executor"),
		store:     store,
		tickets:   tickets,
		publisher: publisher,
	}
}

// Start binds the durable consumer on the reviewed-action subject.
func (e *AIActionExecutor) Start(_ context.Context) error {
	return e.consumer.Bind(conversation.SubjectAIActionReviewed, aiActionExecutorDurable, e.handle)
}

// Stop drains the subscription.
func (e *AIActionExecutor) Stop() { e.consumer.Stop() }

// outcome reports how the JetStream message should be settled.
type outcome int

const (
	// outcomeAck: handled, or a definitive skip — ack so it is not redelivered.
	outcomeAck outcome = iota
	// outcomeRetry: transient failure — do not ack so JetStream redelivers.
	outcomeRetry
)

func (e *AIActionExecutor) handle(msg *nats.Msg) {
	var ev conversation.LifecycleEvent
	if err := json.Unmarshal(msg.Data, &ev); err != nil {
		// Poison message — ack to avoid an infinite redelivery loop.
		log.Printf("[cc-go/ai-action-executor] decode %s: %v", msg.Subject, err)
		_ = msg.Ack()
		return
	}
	switch e.process(context.Background(), ev) {
	case outcomeRetry:
		if err := msg.Nak(); err != nil {
			log.Printf("[cc-go/ai-action-executor] nak: %v", err)
		}
	default:
		if err := msg.Ack(); err != nil {
			log.Printf("[cc-go/ai-action-executor] ack: %v", err)
		}
	}
}

// process applies one reviewed-action event exactly once and reports whether
// the message should be acked. It is the testable core of the executor (no
// NATS required).
func (e *AIActionExecutor) process(ctx context.Context, ev conversation.LifecycleEvent) outcome {
	orgID := strings.TrimSpace(ev.OrgID)
	actionID := stringFromData(ev.Data, "ai_action_id")
	decision := stringFromData(ev.Data, "decision")
	if orgID == "" || actionID == "" {
		log.Printf("[cc-go/ai-action-executor] malformed event (missing org_id/ai_action_id); skipping")
		return outcomeAck
	}
	// Only approvals execute. Rejections (and any other decision) are terminal no-ops.
	if decision != "approved" {
		return outcomeAck
	}

	action, err := e.store.GetAIAction(ctx, orgID, actionID)
	if errors.Is(err, conversation.ErrNotFound) {
		return outcomeAck // action gone — nothing to execute
	}
	if err != nil {
		log.Printf("[cc-go/ai-action-executor] get action %s: %v", actionID, err)
		return outcomeRetry
	}
	// MVP scope: only ticket.classification is executable here.
	if action.Kind != kindTicketClassification {
		return outcomeAck
	}
	// Fast idempotent skip on redelivery: already executed (or never approved).
	if action.Status != "approved" {
		return outcomeAck
	}

	ticket, err := e.store.GetTicketByConversation(ctx, orgID, action.ConversationID)
	if errors.Is(err, conversation.ErrNotFound) {
		// The suggested ticket should exist (RecordTicketClassification creates it).
		// If it is gone we cannot honestly promote anything — skip without claiming.
		log.Printf("[cc-go/ai-action-executor] no ticket for conversation %s; skipping action %s", action.ConversationID, actionID)
		return outcomeAck
	}
	if err != nil {
		log.Printf("[cc-go/ai-action-executor] get ticket for conversation %s: %v", action.ConversationID, err)
		return outcomeRetry
	}

	// Claim FIRST so a duplicate/redelivered event never double-promotes.
	claimed, err := e.store.MarkAIActionExecuted(ctx, orgID, actionID)
	if err != nil {
		log.Printf("[cc-go/ai-action-executor] claim %s: %v", actionID, err)
		return outcomeRetry
	}
	if !claimed {
		return outcomeAck // lost the race / already executed — no double-apply
	}

	if err := e.promote(ctx, orgID, ticket, action.Payload); err != nil {
		// Roll the claim back so a redelivery can retry cleanly rather than
		// leaving the action stranded as executed-but-unapplied.
		if uerr := e.store.UnmarkAIActionExecuted(ctx, orgID, actionID); uerr != nil {
			log.Printf("[cc-go/ai-action-executor] unclaim %s after promote failure: %v", actionID, uerr)
		}
		log.Printf("[cc-go/ai-action-executor] promote ticket %s: %v", ticket.ID, err)
		return outcomeRetry
	}

	_ = e.publisher.Publish(ctx, conversation.SubjectAIActionExecuted, conversation.LifecycleEvent{
		Type:           "ai_action.executed",
		OrgID:          orgID,
		ConversationID: action.ConversationID,
		ActorUserID:    ev.ActorUserID,
		Data: map[string]any{
			"ai_action_id": actionID,
			"kind":         action.Kind,
			"ticket_id":    ticket.ID,
		},
		OccurredAt: time.Now().UTC(),
	})
	return outcomeAck
}

// promote flips the suggested ticket to open and applies the AI's suggested
// routing (carried under payload.suggested_fields), via Service.UpdateTicket.
func (e *AIActionExecutor) promote(ctx context.Context, orgID string, ticket *conversation.Ticket, payload map[string]any) error {
	open := "open"
	input := conversation.UpdateTicketInput{
		OrgID:       orgID,
		TicketID:    ticket.ID,
		Status:      &open,
		ActorUserID: executorActor,
	}
	fields := mapFromData(payload, "suggested_fields")
	applyStringField(fields, "category", &input.Category)
	applyStringField(fields, "priority", &input.Priority)
	applyStringField(fields, "severity", &input.Severity)
	applyStringField(fields, "intent", &input.Intent)
	applyStringField(fields, "team_id", &input.TeamID)
	applyStringField(fields, "team_name", &input.TeamName)
	_, err := e.tickets.UpdateTicket(ctx, input)
	return err
}

func stringFromData(data map[string]any, key string) string {
	if data == nil {
		return ""
	}
	if v, ok := data[key].(string); ok {
		return strings.TrimSpace(v)
	}
	return ""
}

func mapFromData(data map[string]any, key string) map[string]any {
	if data == nil {
		return nil
	}
	if v, ok := data[key].(map[string]any); ok {
		return v
	}
	return nil
}

// applyStringField sets *dest to a heap copy of fields[key] when it is a
// non-empty string, leaving the optional UpdateTicketInput field nil otherwise.
func applyStringField(fields map[string]any, key string, dest **string) {
	if fields == nil {
		return
	}
	if v, ok := fields[key].(string); ok {
		if s := strings.TrimSpace(v); s != "" {
			*dest = &s
		}
	}
}

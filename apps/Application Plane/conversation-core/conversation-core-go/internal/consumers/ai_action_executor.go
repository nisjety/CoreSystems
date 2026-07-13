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
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/integration"
)

const (
	aiActionExecutorDurable  = "conversation-core-ai-action-executor"
	kindTicketClassification = "ticket.classification"
	kindDraftReply           = "draft.reply"
	executorActor            = "ai-action-executor"
	retryableStateAttempts   = 3
	retryableStateTimeout    = 2 * time.Second
)

// ActionStore is the narrow read/claim surface the executor needs.
// *conversation.PGRepository satisfies it.
type ActionStore interface {
	GetAIAction(ctx context.Context, orgID, id string) (*conversation.AIAction, error)
	GetTicketByConversation(ctx context.Context, orgID, conversationID string) (*conversation.Ticket, error)
	GetChannelThreadRefByConversation(ctx context.Context, orgID, conversationID string) (*conversation.ChannelThreadRef, error)
	MarkAIActionExecuted(ctx context.Context, orgID, id string) (bool, error)
	UnmarkAIActionExecuted(ctx context.Context, orgID, id string) error
	ClaimOutboundIntent(ctx context.Context, input conversation.OutboundIntentClaimInput) (*conversation.OutboundIntentClaim, error)
	FinalizeOutboundIntent(ctx context.Context, input conversation.OutboundIntentFinalizeInput) (*conversation.Message, error)
	MarkOutboundIntentOutcome(ctx context.Context, input conversation.OutboundIntentOutcomeInput) error
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

// OutboundSender sends an approved draft.reply through integration-corev2. It is
// the adapter-confirmation point: a reply is only "sent" once Send returns nil.
// *integration.Client satisfies it. A nil OutboundSender disables draft.reply
// execution (so the executor never claims a send it cannot perform).
type OutboundSender interface {
	Send(ctx context.Context, req integration.SendRequest) (*integration.SendResult, error)
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
	sender    OutboundSender
}

// NewAIActionExecutor wires the executor. sender may be nil when no integration
// client is configured; in that case draft.reply actions are skipped (never
// claimed), so the executor never asserts a send it cannot make.
func NewAIActionExecutor(js nats.JetStreamContext, store ActionStore, tickets TicketPromoter, publisher Publisher, sender OutboundSender) *AIActionExecutor {
	return &AIActionExecutor{
		consumer:  NewDurableConsumer(js, "ai-action-executor"),
		store:     store,
		tickets:   tickets,
		publisher: publisher,
		sender:    sender,
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
	// Fast idempotent skip on redelivery: already executed (or never approved).
	if action.Status != "approved" {
		return outcomeAck
	}
	reviewerID := strings.TrimSpace(action.ReviewedBy)
	if action.Kind == kindDraftReply && reviewerID == "" {
		log.Printf("[cc-go/ai-action-executor] approved draft.reply %s has no durable reviewer; skipping", actionID)
		return outcomeAck
	}

	// Dispatch by kind. Each branch reuses the SAME atomic approved→executed
	// claim guard (MarkAIActionExecuted) keyed by the AIAction id, which equals
	// the approval_id, so a redelivered approve never double-applies.
	switch action.Kind {
	case kindTicketClassification:
		return e.executeTicketClassification(ctx, orgID, actionID, action, ev)
	case kindDraftReply:
		return e.executeDraftReply(ctx, orgID, actionID, action, ev)
	default:
		// Unknown kind — not executable here. Terminal no-op.
		return outcomeAck
	}
}

// executeTicketClassification promotes the suggested ticket and applies routing.
func (e *AIActionExecutor) executeTicketClassification(ctx context.Context, orgID, actionID string, action *conversation.AIAction, ev conversation.LifecycleEvent) outcome {
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

// executeDraftReply claims a durable, content-free outbound intent before the
// provider call. Ambiguous outcomes become unknown and are ACKed for operator
// reconciliation; they are never unclaimed or blindly retransmitted.
func (e *AIActionExecutor) executeDraftReply(ctx context.Context, orgID, actionID string, action *conversation.AIAction, ev conversation.LifecycleEvent) outcome {
	reviewerID := strings.TrimSpace(action.ReviewedBy)
	if e.sender == nil {
		// No integration client configured — we cannot send, and must not claim
		// the action as executed. Skip honestly without claiming.
		log.Printf("[cc-go/ai-action-executor] draft.reply skipped: no outbound sender configured (action %s)", actionID)
		return outcomeAck
	}

	// Resolve the send target BEFORE claiming so a missing channel ref skips
	// without burning the claim (mirrors the missing-ticket path).
	ref, err := e.store.GetChannelThreadRefByConversation(ctx, orgID, action.ConversationID)
	if errors.Is(err, conversation.ErrNotFound) {
		log.Printf("[cc-go/ai-action-executor] no channel thread ref for conversation %s; skipping draft.reply %s", action.ConversationID, actionID)
		return outcomeAck
	}
	if err != nil {
		log.Printf("[cc-go/ai-action-executor] get channel ref for conversation %s: %v", action.ConversationID, err)
		return outcomeRetry
	}

	idempotencyKey := "conversation-ai:" + actionID
	intentID := conversation.OutboundIntentID(orgID, idempotencyKey)
	req := buildSendRequest(orgID, reviewerID, actionID, intentID, ref, action.Payload)
	prepared, err := integration.PrepareSend(req)
	if err != nil {
		log.Printf("[cc-go/ai-action-executor] prepare provider effect for draft.reply %s: %v", actionID, err)
		return outcomeAck
	}
	req.PayloadSHA256 = prepared.PayloadSHA256
	fingerprint := conversation.OutboundRequestFingerprint(
		orgID, action.ConversationID, actionID, reviewerID, ref,
		stringFromData(action.Payload, "body_text"), stringFromData(action.Payload, "body_html"),
	)
	claim, err := e.store.ClaimOutboundIntent(ctx, conversation.OutboundIntentClaimInput{
		IntentID: intentID, OrgID: orgID, IdempotencyKey: idempotencyKey, ConversationID: action.ConversationID,
		AIActionID: actionID, RequestFingerprint: fingerprint, Provider: ref.Provider,
		ConnectionID: ref.ConnectionID, ProviderThreadID: ref.ProviderThreadID,
		AuthorizationKind: "human_approved_ai_action", ActorUserID: reviewerID,
		ApprovalID: actionID, ActionID: actionID, Operation: prepared.Operation, PayloadSHA256: prepared.PayloadSHA256,
	})
	if err != nil {
		if errors.Is(err, conversation.ErrConflict) {
			log.Printf("[cc-go/ai-action-executor] conflicting outbound intent for draft.reply %s; skipping", actionID)
			return outcomeAck
		}
		log.Printf("[cc-go/ai-action-executor] claim outbound intent %s: %v", actionID, err)
		return outcomeRetry
	}
	if !claim.Claimed {
		return outcomeAck
	}

	result, sendErr := e.sender.Send(ctx, req)
	if sendErr != nil {
		errorCode := integration.ErrorCode(sendErr)
		if integration.IsSafeToRetry(sendErr) {
			outcomeInput := conversation.OutboundIntentOutcomeInput{
				OrgID: orgID, IdempotencyKey: idempotencyKey, AIActionID: actionID,
				Status: conversation.OutboundIntentRetryable, ErrorCode: errorCode,
			}
			if outcomeErr := e.persistRetryablePreProviderOutcome(ctx, outcomeInput); outcomeErr != nil {
				log.Printf("[cc-go/ai-action-executor] retryable state unavailable for draft.reply %s; reconciliation required: %v", actionID, outcomeErr)
				e.publishSendOutcome(ctx, conversation.SubjectAIActionSendUnknown, conversation.OutboundIntentUnknown, orgID, action, ev, "retryable_state_persistence_failed")
				return outcomeAck
			}
			return outcomeRetry
		}
		status := conversation.OutboundIntentUnknown
		subject := conversation.SubjectAIActionSendUnknown
		if integration.IsTerminal(sendErr) {
			status = conversation.OutboundIntentFailed
			subject = conversation.SubjectAIActionSendFailed
		}
		if outcomeErr := e.store.MarkOutboundIntentOutcome(ctx, conversation.OutboundIntentOutcomeInput{
			OrgID: orgID, IdempotencyKey: idempotencyKey, AIActionID: actionID,
			Status: status, ErrorCode: errorCode,
		}); outcomeErr != nil {
			log.Printf("[cc-go/ai-action-executor] record %s outcome for draft.reply %s: %v", status, actionID, outcomeErr)
		}
		e.publishSendOutcome(ctx, subject, status, orgID, action, ev, errorCode)
		log.Printf("[cc-go/ai-action-executor] %s send outcome for draft.reply %s (code=%s)", status, actionID, errorCode)
		return outcomeAck
	}

	providerMessageID := ""
	if result != nil {
		providerMessageID = result.ProviderMessageID
	}
	message, finalizeErr := e.store.FinalizeOutboundIntent(ctx, conversation.OutboundIntentFinalizeInput{
		OrgID: orgID, IdempotencyKey: idempotencyKey, RequestFingerprint: fingerprint,
		AIActionID: actionID, ProviderMessageID: providerMessageID,
		Message: conversation.AddMessageInput{
			OrgID: orgID, ConversationID: action.ConversationID, ActorUserID: reviewerID,
			BodyText: stringFromData(action.Payload, "body_text"), BodyHTML: stringFromData(action.Payload, "body_html"),
			Direction: conversation.DirectionOutbound, IdempotencyKey: idempotencyKey, OccurredAt: time.Now().UTC(),
		},
	})
	if finalizeErr != nil {
		if outcomeErr := e.store.MarkOutboundIntentOutcome(ctx, conversation.OutboundIntentOutcomeInput{
			OrgID: orgID, IdempotencyKey: idempotencyKey, AIActionID: actionID,
			Status: conversation.OutboundIntentUnknown, ErrorCode: "local_finalize_failed",
		}); outcomeErr != nil {
			log.Printf("[cc-go/ai-action-executor] mark finalization unknown for draft.reply %s: %v", actionID, outcomeErr)
		}
		e.publishSendOutcome(ctx, conversation.SubjectAIActionSendUnknown, conversation.OutboundIntentUnknown, orgID, action, ev, "local_finalize_failed")
		return outcomeAck
	}
	_ = e.publisher.Publish(ctx, conversation.SubjectAIActionExecuted, conversation.LifecycleEvent{
		Type:           "ai_action.executed",
		OrgID:          orgID,
		ConversationID: action.ConversationID,
		ActorUserID:    reviewerID,
		Data: map[string]any{
			"ai_action_id":        actionID,
			"kind":                action.Kind,
			"message_id":          message.ID,
			"provider":            ref.Provider,
			"connection_id":       ref.ConnectionID,
			"provider_message_id": providerMessageID,
		},
		OccurredAt: time.Now().UTC(),
	})
	return outcomeAck
}

// persistRetryablePreProviderOutcome makes a bounded effort to persist the only
// state that permits an automatic provider retry. If it cannot be recorded,
// the caller must ACK as reconciliation-required; a lingering `sending` intent
// then prevents redelivery from calling the provider.
func (e *AIActionExecutor) persistRetryablePreProviderOutcome(ctx context.Context, input conversation.OutboundIntentOutcomeInput) error {
	retryContext, cancel := context.WithTimeout(ctx, retryableStateTimeout)
	defer cancel()
	var lastErr error
	for range retryableStateAttempts {
		if err := e.store.MarkOutboundIntentOutcome(retryContext, input); err == nil {
			return nil
		} else {
			lastErr = err
		}
		if retryContext.Err() != nil {
			break
		}
	}
	return lastErr
}

func (e *AIActionExecutor) publishSendOutcome(ctx context.Context, subject, status, orgID string, action *conversation.AIAction, ev conversation.LifecycleEvent, errorCode string) {
	_ = e.publisher.Publish(ctx, subject, conversation.LifecycleEvent{
		Type:           "ai_action.send_" + status,
		OrgID:          orgID,
		ConversationID: action.ConversationID,
		ActorUserID:    strings.TrimSpace(action.ReviewedBy),
		Data: map[string]any{
			"ai_action_id": action.ID,
			"kind":         action.Kind,
			"status":       status,
			"error_code":   errorCode,
		},
		OccurredAt: time.Now().UTC(),
	})
}

// buildSendRequest maps the approved draft.reply payload + resolved channel ref
// into an integration send request.
func buildSendRequest(orgID, reviewerID, actionID, intentID string, ref *conversation.ChannelThreadRef, payload map[string]any) integration.SendRequest {
	return integration.SendRequest{
		OrgID:             orgID,
		ActorUserID:       reviewerID,
		Provider:          ref.Provider,
		ConnectionID:      ref.ConnectionID,
		ProviderThreadID:  ref.ProviderThreadID,
		BodyText:          stringFromData(payload, "body_text"),
		BodyHTML:          stringFromData(payload, "body_html"),
		Subject:           stringFromData(payload, "subject"),
		To:                stringSliceFromData(payload, "to"),
		AuthorizationKind: "human_approved_ai_action",
		AuthorizationID:   intentID,
		ApprovalID:        actionID,
		ActionID:          actionID,
		IdempotencyKey:    "conversation-ai:" + actionID,
	}
}

// stringSliceFromData reads a []string from payload[key], tolerating a JSON
// array of strings (decoded as []any).
func stringSliceFromData(data map[string]any, key string) []string {
	if data == nil {
		return nil
	}
	raw, ok := data[key].([]any)
	if !ok {
		return nil
	}
	out := make([]string, 0, len(raw))
	for _, v := range raw {
		if s, ok := v.(string); ok {
			if trimmed := strings.TrimSpace(s); trimmed != "" {
				out = append(out, trimmed)
			}
		}
	}
	if len(out) == 0 {
		return nil
	}
	return out
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

package conversation

import (
	"context"
	"errors"
	"fmt"
	"log"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/integration"
)

// OutboundSender delivers a human agent's reply to the customer through
// integration-corev2. *integration.Client satisfies it. A nil sender disables
// outbound delivery: replies are stored but not sent (used in tests and when no
// integration client is configured), so the Service never claims a send it
// cannot perform.
type OutboundSender interface {
	Send(ctx context.Context, req integration.SendRequest) (*integration.SendResult, error)
}

type Service struct {
	repository Repository
	publisher  EventPublisher
	sender     OutboundSender
	now        func() time.Time
}

type Option func(*Service)

func WithNow(now func() time.Time) Option {
	return func(s *Service) {
		if now != nil {
			s.now = now
		}
	}
}

// WithSender wires the outbound delivery client used to actually send human
// agent replies to channel-backed conversations (whatsapp, messenger, …).
func WithSender(sender OutboundSender) Option {
	return func(s *Service) {
		s.sender = sender
	}
}

func NewService(repository Repository, publisher EventPublisher, opts ...Option) *Service {
	service := &Service{
		repository: repository,
		publisher:  publisher,
		now:        time.Now,
	}
	for _, opt := range opts {
		opt(service)
	}
	return service
}

func (s *Service) ListInboxes(ctx context.Context, orgID string) ([]Inbox, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListInboxes(ctx, orgID)
}

func (s *Service) ListConversations(ctx context.Context, filter ListFilter) ([]ConversationSummary, error) {
	filter.OrgID = strings.TrimSpace(filter.OrgID)
	filter.InboxID = strings.TrimSpace(filter.InboxID)
	filter.Status = normalizeStatus(filter.Status)
	filter.Assigned = strings.TrimSpace(filter.Assigned)
	filter.Channel = strings.TrimSpace(filter.Channel)
	filter.Query = strings.TrimSpace(filter.Query)
	if filter.OrgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	return s.repository.ListConversations(ctx, filter)
}

func (s *Service) GetConversation(ctx context.Context, orgID, conversationID string) (*ConversationDetail, error) {
	orgID = strings.TrimSpace(orgID)
	conversationID = strings.TrimSpace(conversationID)
	if orgID == "" || conversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	return s.repository.GetConversation(ctx, orgID, conversationID)
}

func (s *Service) IngestEvent(ctx context.Context, event InboundEvent) (*StoredEventResult, error) {
	event = normalizeInboundEvent(event, s.now)
	if err := validateInboundEvent(event); err != nil {
		return nil, err
	}
	result, err := s.repository.StoreInboundEvent(ctx, event)
	if err != nil {
		return nil, fmt.Errorf("store inbound event: %w", err)
	}
	if result != nil && result.Created {
		s.publish(ctx, SubjectMessageReceived, result.Detail, result.Message, "", map[string]any{
			"provider":            event.Provider,
			"provider_event_id":   event.ProviderEventID,
			"provider_message_id": event.ProviderMessageID,
		})
	}
	return result, nil
}

func (s *Service) AddMessage(ctx context.Context, input AddMessageInput) (*Message, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	input.ActorName = strings.TrimSpace(input.ActorName)
	input.ActorEmail = strings.TrimSpace(input.ActorEmail)
	input.BodyText = strings.TrimSpace(input.BodyText)
	input.BodyHTML = strings.TrimSpace(input.BodyHTML)
	if strings.TrimSpace(input.Direction) == "" {
		input.Direction = DirectionOutbound
	}
	if input.OccurredAt.IsZero() {
		input.OccurredAt = s.now().UTC()
	}
	if input.OrgID == "" || input.ConversationID == "" || input.BodyText == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and body_text are required", ErrInvalidInput)
	}
	// Human replies (outbound, non-internal) to a conversation backed by a
	// channel with a real send operation must actually reach the customer. Send
	// FIRST, then persist — so a send failure surfaces a real error and no
	// phantom "sent" row is stored, instead of the previous false success where
	// the row was written but nothing was delivered. Internal notes, store-only
	// conversations (no channel ref) and channels with no send op are unaffected.
	if !input.Internal && input.Direction == DirectionOutbound && s.sender != nil {
		provider, providerMessageID, err := s.deliverReply(ctx, input)
		if err != nil {
			return nil, err
		}
		input.Provider = provider
		input.ProviderMessageID = providerMessageID
	}
	message, err := s.repository.AddMessage(ctx, input)
	if err != nil {
		return nil, err
	}
	subject := SubjectMessageSent
	if input.Internal {
		subject = SubjectNoteCreated
	}
	s.publish(ctx, subject, &ConversationDetail{ConversationSummary: ConversationSummary{ID: input.ConversationID, OrgID: input.OrgID}}, message, input.ActorUserID, nil)
	return message, nil
}

// deliverReply sends a human agent's outbound reply to the customer through
// integration-corev2 when the conversation is backed by a channel with a real
// send operation. It returns the provider + provider message id to record on
// the stored message. It is deliberately conservative about NOT sending:
//   - a conversation with no channel thread ref (ErrNotFound) is store-only
//     (returns "", "", nil) — preserving prior behavior for internal-only or
//     unbound conversations;
//   - a channel whose provider has no send mapping (e.g. a plain email inbox)
//     is also store-only, so we never turn an undeliverable channel into a hard
//     error for the agent.
//
// A send that IS attempted but fails returns ErrSendFailed (wrapping the
// underlying cause) so the caller never persists a message the customer never
// received and the Inbox never shows a phantom "Reply sent".
func (s *Service) deliverReply(ctx context.Context, input AddMessageInput) (provider, providerMessageID string, err error) {
	ref, refErr := s.repository.GetChannelThreadRefByConversation(ctx, input.OrgID, input.ConversationID)
	if errors.Is(refErr, ErrNotFound) {
		return "", "", nil
	}
	if refErr != nil {
		return "", "", refErr
	}
	if !integration.SupportsSend(ref.Provider) {
		return "", "", nil
	}
	result, sendErr := s.sender.Send(ctx, integration.SendRequest{
		OrgID:            input.OrgID,
		ActorUserID:      input.ActorUserID,
		Provider:         ref.Provider,
		ConnectionID:     ref.ConnectionID,
		ProviderThreadID: ref.ProviderThreadID,
		BodyText:         input.BodyText,
		BodyHTML:         input.BodyHTML,
	})
	if sendErr != nil {
		return "", "", fmt.Errorf("%w: %v", ErrSendFailed, sendErr)
	}
	if result != nil {
		providerMessageID = result.ProviderMessageID
	}
	return ref.Provider, providerMessageID, nil
}

func (s *Service) UpdateStatus(ctx context.Context, input StatusUpdate) (*ConversationDetail, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	input.Status = normalizeStatus(input.Status)
	if input.OrgID == "" || input.ConversationID == "" || input.Status == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and status are required", ErrInvalidInput)
	}
	detail, err := s.repository.UpdateStatus(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectStatusChanged, detail, nil, input.ActorUserID, map[string]any{"status": input.Status})
	return detail, nil
}

func (s *Service) UpdateAssignment(ctx context.Context, input AssignmentUpdate) (*ConversationDetail, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.AssigneeUserID = strings.TrimSpace(input.AssigneeUserID)
	input.AssigneeName = strings.TrimSpace(input.AssigneeName)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.ConversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	detail, err := s.repository.UpdateAssignment(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectAssignmentChanged, detail, nil, input.ActorUserID, map[string]any{
		"assignee_user_id": input.AssigneeUserID,
		"assignee_name":    input.AssigneeName,
	})
	return detail, nil
}

func (s *Service) AddTag(ctx context.Context, orgID, conversationID, tag, actorUserID string) (*ConversationDetail, error) {
	tag = strings.TrimSpace(tag)
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(conversationID) == "" || tag == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and tag are required", ErrInvalidInput)
	}
	detail, err := s.repository.AddTag(ctx, orgID, conversationID, tag)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectTagAdded, detail, nil, actorUserID, map[string]any{"tag": tag})
	return detail, nil
}

func (s *Service) RemoveTag(ctx context.Context, orgID, conversationID, tag, actorUserID string) (*ConversationDetail, error) {
	tag = strings.TrimSpace(tag)
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(conversationID) == "" || tag == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and tag are required", ErrInvalidInput)
	}
	detail, err := s.repository.RemoveTag(ctx, orgID, conversationID, tag)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectTagRemoved, detail, nil, actorUserID, map[string]any{"tag": tag})
	return detail, nil
}

func (s *Service) ReviewAIAction(ctx context.Context, input AIActionReview) error {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.AIActionID = strings.TrimSpace(input.AIActionID)
	input.ReviewerID = strings.TrimSpace(input.ReviewerID)
	input.Decision = strings.TrimSpace(input.Decision)
	input.Comment = strings.TrimSpace(input.Comment)
	if input.OccurredAt.IsZero() {
		input.OccurredAt = s.now().UTC()
	}
	if input.OrgID == "" || input.AIActionID == "" || input.ReviewerID == "" || input.Decision == "" {
		return fmt.Errorf("%w: org_id, ai_action_id, reviewer_id, and decision are required", ErrInvalidInput)
	}
	if input.Decision != "approved" && input.Decision != "rejected" {
		return fmt.Errorf("%w: decision must be 'approved' or 'rejected'", ErrInvalidInput)
	}
	if err := s.repository.ReviewAIAction(ctx, input); err != nil {
		return err
	}
	s.publish(ctx, SubjectAIActionReviewed, &ConversationDetail{ConversationSummary: ConversationSummary{OrgID: input.OrgID}}, nil, input.ReviewerID, map[string]any{
		"ai_action_id": input.AIActionID,
		"decision":     input.Decision,
	})
	return nil
}

// allowedAIActionKinds is the closed set of kinds a human/hook may propose via
// CreateAIAction. Keep it explicit so an arbitrary kind can never be queued and
// later "executed" — only draft.reply is actable through the act-leg today.
var allowedAIActionKinds = map[string]bool{
	"draft.reply": true,
}

// CreateAIAction validates and persists a model-proposed action into the HITL
// review queue (status 'suggested'). It is the generic propose path shared by
// the POST /ai-actions route and the model-proposed consumer. The kind must be
// in the allowlist; org_id and conversation_id are required.
func (s *Service) CreateAIAction(ctx context.Context, input CreateAIActionInput) (*AIAction, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.Kind = strings.TrimSpace(input.Kind)
	input.CreatedBy = strings.TrimSpace(input.CreatedBy)
	if input.OrgID == "" || input.ConversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	if input.Kind == "" {
		return nil, fmt.Errorf("%w: kind is required", ErrInvalidInput)
	}
	if !allowedAIActionKinds[input.Kind] {
		return nil, fmt.Errorf("%w: unsupported action kind %q", ErrInvalidInput, input.Kind)
	}
	if input.Payload == nil {
		input.Payload = map[string]any{}
	}
	return s.repository.CreateAIAction(ctx, input)
}

func (s *Service) ListAIActions(ctx context.Context, filter AIActionListFilter) ([]AIAction, error) {
	filter.OrgID = strings.TrimSpace(filter.OrgID)
	filter.Status = strings.TrimSpace(filter.Status)
	filter.ConversationID = strings.TrimSpace(filter.ConversationID)
	if filter.OrgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	// Default to the review queue (suggested actions awaiting a human decision).
	// "all" is the explicit escape hatch to list every status for the org.
	switch strings.ToLower(filter.Status) {
	case "":
		filter.Status = "suggested"
	case "all":
		filter.Status = ""
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	return s.repository.ListAIActions(ctx, filter)
}

func (s *Service) ListTickets(ctx context.Context, filter TicketListFilter) ([]Ticket, error) {
	filter.OrgID = strings.TrimSpace(filter.OrgID)
	filter.Queue = strings.TrimSpace(filter.Queue)
	filter.Status = normalizeTicketStatus(filter.Status)
	filter.Assigned = strings.TrimSpace(filter.Assigned)
	filter.TeamID = strings.TrimSpace(filter.TeamID)
	filter.Label = strings.TrimSpace(filter.Label)
	filter.Priority = normalizeOptionalTicketPriority(filter.Priority)
	filter.Severity = normalizeOptionalTicketSeverity(filter.Severity)
	filter.SLAState = normalizeSLAState(filter.SLAState)
	filter.Query = strings.TrimSpace(filter.Query)
	if filter.OrgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	return s.repository.ListTickets(ctx, filter)
}

func (s *Service) GetTicket(ctx context.Context, orgID, ticketID string) (*Ticket, error) {
	orgID = strings.TrimSpace(orgID)
	ticketID = strings.TrimSpace(ticketID)
	if orgID == "" || ticketID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket_id are required", ErrInvalidInput)
	}
	return s.repository.GetTicket(ctx, orgID, ticketID)
}

func (s *Service) CreateTicket(ctx context.Context, input CreateTicketInput) (*Ticket, error) {
	input = normalizeCreateTicketInput(input)
	if input.OrgID == "" || input.ConversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	if _, err := s.repository.GetConversation(ctx, input.OrgID, input.ConversationID); err != nil {
		return nil, err
	}
	ticket, err := s.repository.CreateTicket(ctx, input)
	if err != nil {
		return nil, err
	}
	subject := SubjectTicketCreated
	if ticket.Status == "suggested" {
		subject = SubjectTicketSuggested
	}
	s.publishTicket(ctx, subject, ticket, input.ActorUserID)
	return s.evaluateTicketAutomationRules(ctx, "ticket.created", ticket, input.ActorUserID)
}

func (s *Service) UpdateTicket(ctx context.Context, input UpdateTicketInput) (*Ticket, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.TicketID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket_id are required", ErrInvalidInput)
	}
	normalizeStringPtr(input.Status, normalizeTicketStatus)
	normalizeStringPtr(input.Priority, normalizeTicketPriority)
	normalizeStringPtr(input.Severity, normalizeTicketSeverity)
	trimStringPtr(input.Category)
	trimStringPtr(input.Intent)
	trimStringPtr(input.AssigneeUserID)
	trimStringPtr(input.AssigneeName)
	trimStringPtr(input.TeamID)
	trimStringPtr(input.TeamName)
	trimStringPtr(input.Source)
	trimStringPtr(input.AIReason)
	trimStringPtr(input.SLAPolicyID)
	if input.Labels != nil {
		labels := normalizeLabels(*input.Labels)
		input.Labels = &labels
	}
	if input.Status != nil {
		switch *input.Status {
		case "resolved", "closed":
			if input.ResolvedAt == nil {
				resolvedAt := s.now().UTC()
				input.ResolvedAt = &resolvedAt
			}
		case "waiting_customer", "waiting_team":
			if input.WaitingSince == nil {
				waitingSince := s.now().UTC()
				input.WaitingSince = &waitingSince
			}
		}
	}
	ticket, err := s.repository.UpdateTicket(ctx, input)
	if err != nil {
		return nil, err
	}
	subject := SubjectTicketUpdated
	if input.AssigneeUserID != nil || input.AssigneeName != nil || input.TeamID != nil || input.TeamName != nil {
		subject = SubjectTicketAssigned
	}
	if input.Status != nil && (*input.Status == "resolved" || *input.Status == "closed") {
		subject = SubjectTicketResolved
	}
	s.publishTicket(ctx, subject, ticket, input.ActorUserID)
	return s.evaluateTicketAutomationRules(ctx, "ticket.updated", ticket, input.ActorUserID)
}

func (s *Service) LinkTicketResource(ctx context.Context, input LinkTicketResourceInput) (*TicketLinkedResource, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.ResourceKind = strings.TrimSpace(input.ResourceKind)
	input.LinkType = normalizeLinkType(input.LinkType)
	input.ResourceID = strings.TrimSpace(input.ResourceID)
	input.ResourceURL = strings.TrimSpace(input.ResourceURL)
	input.Label = strings.TrimSpace(input.Label)
	input.CreatedByUserID = strings.TrimSpace(input.CreatedByUserID)
	if input.OrgID == "" || input.TicketID == "" || input.ResourceKind == "" {
		return nil, fmt.Errorf("%w: org_id, ticket_id, and resource_kind are required", ErrInvalidInput)
	}
	link, err := s.repository.LinkTicketResource(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectTicketLinked, &ConversationDetail{ConversationSummary: ConversationSummary{
		ID:    link.ConversationID,
		OrgID: link.OrgID,
	}}, nil, input.CreatedByUserID, map[string]any{"link": link})
	return link, nil
}

func (s *Service) RecordTicketClassification(ctx context.Context, input TicketClassificationInput) (*TicketClassification, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.Reason = strings.TrimSpace(input.Reason)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.SuggestedFields == nil {
		input.SuggestedFields = map[string]any{}
	}
	if input.OrgID == "" || input.ConversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	if input.Confidence < 0 {
		input.Confidence = 0
	}
	if input.Confidence > 1 {
		input.Confidence = 1
	}
	input.Outcome = normalizeClassificationOutcome(input.Outcome, input.Confidence, input.SuggestedFields, input.EvidenceMessageIDs)
	payload := map[string]any{
		"outcome":              input.Outcome,
		"confidence":           input.Confidence,
		"reason":               input.Reason,
		"suggested_fields":     input.SuggestedFields,
		"evidence_message_ids": input.EvidenceMessageIDs,
	}
	classification, err := s.repository.RecordTicketClassification(ctx, input, payload)
	if err != nil {
		return nil, err
	}
	if input.Outcome == "no_ticket" {
		return classification, nil
	}
	status := "suggested"
	if input.Outcome == "auto_ticket" {
		status = StatusOpen
	}
	ticket, err := s.CreateTicket(ctx, CreateTicketInput{
		OrgID:          input.OrgID,
		ConversationID: input.ConversationID,
		Status:         status,
		Priority:       stringField(input.SuggestedFields, "priority"),
		Severity:       stringField(input.SuggestedFields, "severity"),
		Category:       stringField(input.SuggestedFields, "category"),
		Intent:         stringField(input.SuggestedFields, "intent"),
		TeamID:         stringField(input.SuggestedFields, "team_id"),
		TeamName:       stringField(input.SuggestedFields, "team_name"),
		DueAt:          timeField(input.SuggestedFields, "due_at"),
		Source:         "ai",
		AIConfidence:   input.Confidence,
		AIReason:       input.Reason,
		CreatedBy:      "ai",
		ActorUserID:    input.ActorUserID,
	})
	if err != nil {
		if errors.Is(err, ErrConflict) {
			existing, lookupErr := s.repository.GetTicketByConversation(ctx, input.OrgID, input.ConversationID)
			if lookupErr != nil {
				return nil, lookupErr
			}
			classification.Ticket = existing
			return classification, nil
		}
		return nil, err
	}
	classification.Ticket = ticket
	return classification, nil
}

func (s *Service) ListTicketViews(ctx context.Context, orgID string) ([]TicketView, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListTicketViews(ctx, orgID)
}

func (s *Service) CreateTicketView(ctx context.Context, input CreateTicketViewInput) (*TicketView, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.Scope = normalizeScopedValue(input.Scope, "org", "org", "user", "team")
	input.OwnerUserID = strings.TrimSpace(input.OwnerUserID)
	input.TeamID = strings.TrimSpace(input.TeamID)
	input.Visibility = normalizeScopedValue(input.Visibility, "sidebar", "sidebar", "hidden")
	input.GroupBy = strings.TrimSpace(input.GroupBy)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.Filter == nil {
		input.Filter = map[string]any{}
	}
	if input.Sort == nil {
		input.Sort = map[string]any{}
	}
	if input.OrgID == "" || input.Name == "" {
		return nil, fmt.Errorf("%w: org_id and name are required", ErrInvalidInput)
	}
	return s.repository.CreateTicketView(ctx, input)
}

func (s *Service) UpdateTicketView(ctx context.Context, input UpdateTicketViewInput) (*TicketView, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ID = strings.TrimSpace(input.ID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	trimStringPtr(input.Name)
	normalizeScopedPtr(input.Scope, "org", "org", "user", "team")
	trimStringPtr(input.OwnerUserID)
	trimStringPtr(input.TeamID)
	normalizeScopedPtr(input.Visibility, "sidebar", "sidebar", "hidden")
	trimStringPtr(input.GroupBy)
	if input.OrgID == "" || input.ID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket view id are required", ErrInvalidInput)
	}
	return s.repository.UpdateTicketView(ctx, input)
}

func (s *Service) ListTicketMacros(ctx context.Context, orgID string) ([]TicketMacro, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListTicketMacros(ctx, orgID)
}

func (s *Service) CreateTicketMacro(ctx context.Context, input CreateTicketMacroInput) (*TicketMacro, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.Description = strings.TrimSpace(input.Description)
	input.Visibility = normalizeScopedValue(input.Visibility, "team", "personal", "team", "org")
	input.TeamID = strings.TrimSpace(input.TeamID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.Actions == nil {
		input.Actions = map[string]any{}
	}
	if input.Conditions == nil {
		input.Conditions = map[string]any{}
	}
	if input.OrgID == "" || input.Name == "" {
		return nil, fmt.Errorf("%w: org_id and name are required", ErrInvalidInput)
	}
	return s.repository.CreateTicketMacro(ctx, input)
}

func (s *Service) UpdateTicketMacro(ctx context.Context, input UpdateTicketMacroInput) (*TicketMacro, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ID = strings.TrimSpace(input.ID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	trimStringPtr(input.Name)
	trimStringPtr(input.Description)
	normalizeScopedPtr(input.Visibility, "team", "personal", "team", "org")
	trimStringPtr(input.TeamID)
	if input.OrgID == "" || input.ID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket macro id are required", ErrInvalidInput)
	}
	return s.repository.UpdateTicketMacro(ctx, input)
}

func (s *Service) RunTicketMacro(ctx context.Context, input TicketMacroRunInput) (*TicketMacroRunResult, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.MacroID = strings.TrimSpace(input.MacroID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.TicketID == "" || input.MacroID == "" {
		return nil, fmt.Errorf("%w: org_id, ticket_id, and macro_id are required", ErrInvalidInput)
	}
	macro, err := s.repository.GetTicketMacro(ctx, input.OrgID, input.MacroID)
	if err != nil {
		return nil, err
	}
	if !macro.Active {
		return nil, fmt.Errorf("%w: ticket macro is inactive", ErrInvalidInput)
	}
	patch := updateTicketInputFromActions(input.OrgID, input.TicketID, input.ActorUserID, macro.Actions)
	ticket, err := s.UpdateTicket(ctx, patch)
	if err != nil {
		return nil, err
	}
	input.Actions = macro.Actions
	if err := s.repository.RecordTicketMacroRun(ctx, input); err != nil {
		return nil, err
	}
	return &TicketMacroRunResult{Ticket: ticket, Macro: *macro}, nil
}

func (s *Service) ListTicketAutomationRules(ctx context.Context, orgID string) ([]TicketAutomationRule, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListTicketAutomationRules(ctx, orgID)
}

func (s *Service) CreateTicketAutomationRule(ctx context.Context, input CreateTicketAutomationRuleInput) (*TicketAutomationRule, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.EventName = normalizeAutomationEvent(input.EventName)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.Conditions == nil {
		input.Conditions = map[string]any{}
	}
	if input.Actions == nil {
		input.Actions = map[string]any{}
	}
	if input.OrgID == "" || input.Name == "" || input.EventName == "" {
		return nil, fmt.Errorf("%w: org_id, name, and event_name are required", ErrInvalidInput)
	}
	return s.repository.CreateTicketAutomationRule(ctx, input)
}

func (s *Service) evaluateTicketAutomationRules(ctx context.Context, eventName string, ticket *Ticket, actorUserID string) (*Ticket, error) {
	if ticket == nil {
		return ticket, nil
	}
	rules, err := s.repository.ListTicketAutomationRules(ctx, ticket.OrgID)
	if err != nil {
		return ticket, err
	}
	current := ticket
	for _, rule := range rules {
		if !rule.Active || rule.EventName != eventName || !ticketAutomationConditionsMatch(rule.Conditions, current) {
			continue
		}
		patch := updateTicketInputFromActions(current.OrgID, current.ID, actorUserID, rule.Actions)
		if !updateTicketInputHasChanges(patch) {
			continue
		}
		updated, err := s.repository.UpdateTicket(ctx, patch)
		if err != nil {
			return current, err
		}
		current = updated
		s.publishTicket(ctx, SubjectTicketUpdated, current, actorUserID)
	}
	return current, nil
}

func (s *Service) UpdateTicketAutomationRule(ctx context.Context, input UpdateTicketAutomationRuleInput) (*TicketAutomationRule, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ID = strings.TrimSpace(input.ID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	trimStringPtr(input.Name)
	normalizeAutomationEventPtr(input.EventName)
	if input.OrgID == "" || input.ID == "" {
		return nil, fmt.Errorf("%w: org_id and automation rule id are required", ErrInvalidInput)
	}
	return s.repository.UpdateTicketAutomationRule(ctx, input)
}

func (s *Service) ListSLAPolicies(ctx context.Context, orgID string) ([]SLAPolicy, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListSLAPolicies(ctx, orgID)
}

func (s *Service) CreateSLAPolicy(ctx context.Context, input CreateSLAPolicyInput) (*SLAPolicy, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.CalendarRef = strings.TrimSpace(input.CalendarRef)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.Conditions == nil {
		input.Conditions = map[string]any{}
	}
	if input.OrgID == "" || input.Name == "" {
		return nil, fmt.Errorf("%w: org_id and name are required", ErrInvalidInput)
	}
	return s.repository.CreateSLAPolicy(ctx, input)
}

func (s *Service) UpdateSLAPolicy(ctx context.Context, input UpdateSLAPolicyInput) (*SLAPolicy, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ID = strings.TrimSpace(input.ID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	trimStringPtr(input.Name)
	trimStringPtr(input.CalendarRef)
	if input.OrgID == "" || input.ID == "" {
		return nil, fmt.Errorf("%w: org_id and SLA policy id are required", ErrInvalidInput)
	}
	return s.repository.UpdateSLAPolicy(ctx, input)
}

func (s *Service) CreateTicketChecklist(ctx context.Context, input CreateTicketChecklistInput) (*TicketChecklist, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.Name = strings.TrimSpace(input.Name)
	input.TemplateID = strings.TrimSpace(input.TemplateID)
	input.CreatedByUserID = strings.TrimSpace(input.CreatedByUserID)
	if input.Name == "" {
		input.Name = "Support checklist"
	}
	input.Items = normalizeLabels(input.Items)
	if input.OrgID == "" || input.TicketID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket_id are required", ErrInvalidInput)
	}
	return s.repository.CreateTicketChecklist(ctx, input)
}

func (s *Service) UpdateTicketChecklistItem(ctx context.Context, input UpdateTicketChecklistItemInput) (*TicketChecklist, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.ChecklistID = strings.TrimSpace(input.ChecklistID)
	input.ItemID = strings.TrimSpace(input.ItemID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.TicketID == "" || input.ChecklistID == "" || input.ItemID == "" {
		return nil, fmt.Errorf("%w: org_id, ticket_id, checklist_id, and item_id are required", ErrInvalidInput)
	}
	return s.repository.UpdateTicketChecklistItem(ctx, input)
}

func (s *Service) publishTicket(ctx context.Context, subject string, ticket *Ticket, actorUserID string) {
	if ticket == nil {
		return
	}
	s.publish(ctx, subject, &ConversationDetail{ConversationSummary: ConversationSummary{
		ID:    ticket.ConversationID,
		OrgID: ticket.OrgID,
	}}, nil, actorUserID, map[string]any{"ticket": ticket})
}

func (s *Service) publish(ctx context.Context, subject string, detail *ConversationDetail, message *Message, actorUserID string, data map[string]any) {
	if s.publisher == nil {
		return
	}
	orgID := ""
	conversationID := ""
	if detail != nil {
		orgID = detail.OrgID
		conversationID = detail.ID
	}
	if orgID == "" && message != nil {
		orgID = message.OrgID
	}
	if conversationID == "" && message != nil {
		conversationID = message.ConversationID
	}
	messageID := ""
	if message != nil {
		messageID = message.ID
	}
	if data == nil {
		data = map[string]any{}
	}
	if detail != nil && detail.ID != "" {
		data["conversation"] = detail.ConversationSummary
	}
	if message != nil {
		data["message"] = message
	}
	eventType := strings.TrimPrefix(subject, "velion.application.conversation.")
	if err := s.publisher.Publish(ctx, subject, LifecycleEvent{
		ID:             newID("evt"),
		Type:           eventType,
		OrgID:          orgID,
		ConversationID: conversationID,
		MessageID:      messageID,
		ActorUserID:    actorUserID,
		Data:           data,
		OccurredAt:     s.now().UTC(),
	}); err != nil {
		log.Printf("conversation-core-go: publish %s: %v", subject, err)
	}
}

func normalizeInboundEvent(event InboundEvent, now func() time.Time) InboundEvent {
	event.IDempotencyKey = strings.TrimSpace(event.IDempotencyKey)
	event.OrgID = strings.TrimSpace(event.OrgID)
	event.ConnectionID = strings.TrimSpace(event.ConnectionID)
	event.Provider = strings.TrimSpace(strings.ToLower(event.Provider))
	if event.Provider == "" {
		event.Provider = "email"
	}
	event.ProviderEventID = strings.TrimSpace(event.ProviderEventID)
	event.ProviderMessageID = strings.TrimSpace(event.ProviderMessageID)
	event.ProviderThreadID = strings.TrimSpace(event.ProviderThreadID)
	event.Direction = strings.TrimSpace(event.Direction)
	if event.Direction == "" {
		event.Direction = DirectionInbound
	}
	event.Subject = strings.TrimSpace(event.Subject)
	event.BodyText = strings.TrimSpace(event.BodyText)
	event.BodyHTML = strings.TrimSpace(event.BodyHTML)
	event.From.Name = strings.TrimSpace(event.From.Name)
	event.From.Email = strings.ToLower(strings.TrimSpace(event.From.Email))
	if event.OccurredAt.IsZero() {
		event.OccurredAt = now().UTC()
	} else {
		event.OccurredAt = event.OccurredAt.UTC()
	}
	if event.IDempotencyKey == "" {
		parts := []string{event.OrgID, event.Provider, event.ProviderEventID, event.ProviderMessageID, event.ProviderThreadID}
		event.IDempotencyKey = strings.Join(parts, ":")
	}
	return event
}

func validateInboundEvent(event InboundEvent) error {
	if event.OrgID == "" {
		return fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	if event.IDempotencyKey == "" || event.IDempotencyKey == ":::" {
		return fmt.Errorf("%w: idempotency_key or provider refs are required", ErrInvalidInput)
	}
	if event.Direction != DirectionInbound && event.Direction != DirectionOutbound {
		return fmt.Errorf("%w: direction must be inbound or outbound", ErrInvalidInput)
	}
	if event.Subject == "" {
		event.Subject = "(no subject)"
	}
	if event.BodyText == "" && event.BodyHTML == "" {
		return fmt.Errorf("%w: body_text or body_html is required", ErrInvalidInput)
	}
	if event.From.Email == "" && event.From.Name == "" {
		return fmt.Errorf("%w: sender is required", ErrInvalidInput)
	}
	return nil
}

func normalizeStatus(status string) string {
	switch strings.ToLower(strings.TrimSpace(status)) {
	case "new", "open":
		return StatusOpen
	case "pending", "pending reminder":
		return StatusPending
	case "solved", "closed":
		return StatusSolved
	case "archived":
		return StatusClosed
	default:
		return strings.ToLower(strings.TrimSpace(status))
	}
}

func normalizeCreateTicketInput(input CreateTicketInput) CreateTicketInput {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.Status = normalizeTicketStatus(input.Status)
	if input.Status == "" {
		input.Status = StatusOpen
	}
	input.Priority = normalizeTicketPriority(input.Priority)
	input.Severity = normalizeTicketSeverity(input.Severity)
	input.Category = strings.TrimSpace(input.Category)
	input.Intent = strings.TrimSpace(input.Intent)
	input.AssigneeUserID = strings.TrimSpace(input.AssigneeUserID)
	input.AssigneeName = strings.TrimSpace(input.AssigneeName)
	input.TeamID = strings.TrimSpace(input.TeamID)
	input.TeamName = strings.TrimSpace(input.TeamName)
	input.Source = strings.TrimSpace(input.Source)
	if input.Source == "" {
		input.Source = "manual"
	}
	input.AIReason = strings.TrimSpace(input.AIReason)
	input.CreatedBy = strings.TrimSpace(input.CreatedBy)
	if input.CreatedBy == "" {
		input.CreatedBy = input.ActorUserID
	}
	input.SLAPolicyID = strings.TrimSpace(input.SLAPolicyID)
	input.Labels = normalizeLabels(input.Labels)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.AIConfidence < 0 {
		input.AIConfidence = 0
	}
	if input.AIConfidence > 1 {
		input.AIConfidence = 1
	}
	switch input.Status {
	case "resolved", "closed":
		if input.ResolvedAt == nil {
			resolvedAt := time.Now().UTC()
			input.ResolvedAt = &resolvedAt
		}
	case "waiting_customer", "waiting_team":
		if input.WaitingSince == nil {
			waitingSince := time.Now().UTC()
			input.WaitingSince = &waitingSince
		}
	}
	return input
}

func normalizeTicketStatus(status string) string {
	switch strings.ToLower(strings.TrimSpace(status)) {
	case "":
		return ""
	case "new", "open":
		return StatusOpen
	case "suggested", "suggestion":
		return "suggested"
	case "pending", "waiting", "waiting_customer", "waiting-customer":
		return "waiting_customer"
	case "waiting_team", "waiting-team":
		return "waiting_team"
	case "snoozed", "snooze":
		return "snoozed"
	case "escalated":
		return "escalated"
	case "resolved", "solved":
		return "resolved"
	case "closed":
		return "closed"
	default:
		return strings.ToLower(strings.TrimSpace(status))
	}
}

func normalizeOptionalTicketPriority(priority string) string {
	value := strings.ToLower(strings.TrimSpace(priority))
	if value == "" {
		return ""
	}
	return normalizeTicketPriority(value)
}

func normalizeTicketPriority(priority string) string {
	switch strings.ToLower(strings.TrimSpace(priority)) {
	case "low", "normal", "high", "urgent":
		return strings.ToLower(strings.TrimSpace(priority))
	default:
		return "normal"
	}
}

func normalizeOptionalTicketSeverity(severity string) string {
	value := strings.ToLower(strings.TrimSpace(severity))
	if value == "" {
		return ""
	}
	return normalizeTicketSeverity(value)
}

func normalizeTicketSeverity(severity string) string {
	switch strings.ToLower(strings.TrimSpace(severity)) {
	case "low", "medium", "high", "critical":
		return strings.ToLower(strings.TrimSpace(severity))
	default:
		return "medium"
	}
}

func normalizeClassificationOutcome(outcome string, confidence float64, fields map[string]any, evidence []string) string {
	requested := strings.ToLower(strings.TrimSpace(outcome))
	category := stringField(fields, "category")
	if category == "" {
		category = stringField(fields, "intent")
	}
	if requested == "auto_ticket" && canAutoCreateTicket(confidence, category, evidence) {
		return "auto_ticket"
	}
	if requested == "suggest_ticket" || requested == "suggested" {
		return "suggest_ticket"
	}
	if requested == "no_ticket" {
		return "no_ticket"
	}
	if confidence < 0.60 {
		return "no_ticket"
	}
	if canAutoCreateTicket(confidence, category, evidence) {
		return "auto_ticket"
	}
	return "suggest_ticket"
}

func canAutoCreateTicket(confidence float64, category string, evidence []string) bool {
	return confidence >= 0.90 && len(evidence) > 0 && !isSensitiveTicketCategory(category)
}

func isSensitiveTicketCategory(category string) bool {
	normalized := strings.ToLower(strings.TrimSpace(category))
	for _, sensitive := range []string{"legal", "security", "abuse", "payment dispute", "medical", "regulated"} {
		if strings.Contains(normalized, sensitive) {
			return true
		}
	}
	return false
}

func normalizeSLAState(value string) string {
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "ok", "risk", "breached":
		return strings.ToLower(strings.TrimSpace(value))
	default:
		return ""
	}
}

func normalizeLinkType(value string) string {
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "parent", "child", "related", "external":
		return strings.ToLower(strings.TrimSpace(value))
	default:
		return "normal"
	}
}

func normalizeLabels(values []string) []string {
	if len(values) == 0 {
		return []string{}
	}
	seen := map[string]struct{}{}
	labels := make([]string, 0, len(values))
	for _, value := range values {
		label := strings.TrimSpace(value)
		if label == "" {
			continue
		}
		key := strings.ToLower(label)
		if _, ok := seen[key]; ok {
			continue
		}
		seen[key] = struct{}{}
		labels = append(labels, label)
	}
	return labels
}

func normalizeScopedValue(value, fallback string, allowed ...string) string {
	normalized := strings.ToLower(strings.TrimSpace(value))
	for _, candidate := range allowed {
		if normalized == candidate {
			return normalized
		}
	}
	return fallback
}

func normalizeScopedPtr(value *string, fallback string, allowed ...string) {
	if value == nil {
		return
	}
	*value = normalizeScopedValue(*value, fallback, allowed...)
}

func normalizeAutomationEvent(value string) string {
	normalized := strings.ToLower(strings.TrimSpace(value))
	switch normalized {
	case "ticket.created", "ticket.updated", "message.received", "sla.risk", "customer.replied", "ticket.resolved":
		return normalized
	default:
		return ""
	}
}

func normalizeAutomationEventPtr(value *string) {
	if value == nil {
		return
	}
	*value = normalizeAutomationEvent(*value)
}

func updateTicketInputFromActions(orgID, ticketID, actorUserID string, actions map[string]any) UpdateTicketInput {
	input := UpdateTicketInput{
		OrgID:       orgID,
		TicketID:    ticketID,
		ActorUserID: actorUserID,
	}
	if status := stringField(actions, "status"); status != "" {
		input.Status = &status
	}
	if priority := stringField(actions, "priority"); priority != "" {
		input.Priority = &priority
	}
	if severity := stringField(actions, "severity"); severity != "" {
		input.Severity = &severity
	}
	if category := stringField(actions, "category"); category != "" {
		input.Category = &category
	}
	if intent := stringField(actions, "intent"); intent != "" {
		input.Intent = &intent
	}
	if assigneeUserID := stringField(actions, "assignee_user_id"); assigneeUserID != "" {
		input.AssigneeUserID = &assigneeUserID
	}
	if assigneeName := stringField(actions, "assignee_name"); assigneeName != "" {
		input.AssigneeName = &assigneeName
	}
	if teamID := stringField(actions, "team_id"); teamID != "" {
		input.TeamID = &teamID
	}
	if teamName := stringField(actions, "team_name"); teamName != "" {
		input.TeamName = &teamName
	}
	if slaPolicyID := stringField(actions, "sla_policy_id"); slaPolicyID != "" {
		input.SLAPolicyID = &slaPolicyID
	}
	if labels := stringSliceField(actions, "labels"); len(labels) > 0 {
		input.Labels = &labels
	}
	if dueAt := timeField(actions, "due_at"); dueAt != nil {
		input.DueAt = dueAt
	}
	if snoozedUntil := timeField(actions, "snoozed_until"); snoozedUntil != nil {
		input.SnoozedUntil = snoozedUntil
	}
	return input
}

func updateTicketInputHasChanges(input UpdateTicketInput) bool {
	return input.Status != nil ||
		input.Priority != nil ||
		input.Severity != nil ||
		input.Category != nil ||
		input.Intent != nil ||
		input.AssigneeUserID != nil ||
		input.AssigneeName != nil ||
		input.TeamID != nil ||
		input.TeamName != nil ||
		input.DueAt != nil ||
		input.Source != nil ||
		input.AIConfidence != nil ||
		input.AIReason != nil ||
		input.WaitingSince != nil ||
		input.LastCustomerReplyAt != nil ||
		input.FirstResponseAt != nil ||
		input.ResolvedAt != nil ||
		input.SnoozedUntil != nil ||
		input.SLAPolicyID != nil ||
		input.EscalationAt != nil ||
		input.Labels != nil
}

func ticketAutomationConditionsMatch(conditions map[string]any, ticket *Ticket) bool {
	if len(conditions) == 0 {
		return true
	}
	values := map[string]string{
		"status":    ticket.Status,
		"priority":  ticket.Priority,
		"severity":  ticket.Severity,
		"category":  ticket.Category,
		"intent":    ticket.Intent,
		"team_id":   ticket.TeamID,
		"team_name": ticket.TeamName,
		"sla_state": ticket.SLAState,
		"assignee":  ticket.AssigneeUserID,
		"source":    ticket.Source,
	}
	for key, expected := range conditions {
		if key == "label" || key == "labels" {
			if !conditionMatchesAnyLabel(expected, ticket.Labels) {
				return false
			}
			continue
		}
		actual, ok := values[key]
		if !ok {
			continue
		}
		if !conditionMatchesString(expected, actual) {
			return false
		}
	}
	return true
}

func conditionMatchesAnyLabel(expected any, labels []string) bool {
	for _, label := range labels {
		if conditionMatchesString(expected, label) {
			return true
		}
	}
	return false
}

func conditionMatchesString(expected any, actual string) bool {
	actual = strings.ToLower(strings.TrimSpace(actual))
	switch value := expected.(type) {
	case string:
		return strings.ToLower(strings.TrimSpace(value)) == actual
	case []string:
		for _, item := range value {
			if strings.ToLower(strings.TrimSpace(item)) == actual {
				return true
			}
		}
	case []any:
		for _, item := range value {
			if text, ok := item.(string); ok && strings.ToLower(strings.TrimSpace(text)) == actual {
				return true
			}
		}
	}
	return false
}

func normalizeStringPtr(value *string, normalize func(string) string) {
	if value == nil {
		return
	}
	*value = normalize(*value)
}

func trimStringPtr(value *string) {
	if value == nil {
		return
	}
	*value = strings.TrimSpace(*value)
}

func stringField(fields map[string]any, key string) string {
	if fields == nil {
		return ""
	}
	if value, ok := fields[key].(string); ok {
		return strings.TrimSpace(value)
	}
	return ""
}

func stringSliceField(fields map[string]any, key string) []string {
	if fields == nil {
		return nil
	}
	switch value := fields[key].(type) {
	case []string:
		return normalizeLabels(value)
	case []any:
		labels := make([]string, 0, len(value))
		for _, item := range value {
			if label, ok := item.(string); ok {
				labels = append(labels, label)
			}
		}
		return normalizeLabels(labels)
	case string:
		return normalizeLabels(strings.Split(value, ","))
	default:
		return nil
	}
}

func timeField(fields map[string]any, key string) *time.Time {
	value := stringField(fields, key)
	if value == "" {
		return nil
	}
	parsed, err := time.Parse(time.RFC3339, value)
	if err != nil {
		return nil
	}
	utc := parsed.UTC()
	return &utc
}

func IsInvalidInput(err error) bool {
	return errors.Is(err, ErrInvalidInput)
}

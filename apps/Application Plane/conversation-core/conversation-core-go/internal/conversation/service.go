package conversation

import (
	"context"
	"errors"
	"fmt"
	"log"
	"strings"
	"time"
)

type Service struct {
	repository Repository
	publisher  EventPublisher
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
	if err := s.repository.ReviewAIAction(ctx, input); err != nil {
		return err
	}
	s.publish(ctx, SubjectAIActionReviewed, &ConversationDetail{ConversationSummary: ConversationSummary{OrgID: input.OrgID}}, nil, input.ReviewerID, map[string]any{
		"ai_action_id": input.AIActionID,
		"decision":     input.Decision,
	})
	return nil
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

func IsInvalidInput(err error) bool {
	return errors.Is(err, ErrInvalidInput)
}

package conversation

import (
	"context"
	"errors"
	"testing"
	"time"
)

type fakeRepository struct {
	inboxes       []Inbox
	conversations []ConversationSummary
	details       map[string]*ConversationDetail
	stored        map[string]*StoredEventResult
	lastMessage   AddMessageInput
	statusUpdate  StatusUpdate
}

func newFakeRepository() *fakeRepository {
	return &fakeRepository{
		details: make(map[string]*ConversationDetail),
		stored:  make(map[string]*StoredEventResult),
	}
}

func (f *fakeRepository) ListInboxes(_ context.Context, _ string) ([]Inbox, error) {
	return f.inboxes, nil
}

func (f *fakeRepository) ListConversations(_ context.Context, _ ListFilter) ([]ConversationSummary, error) {
	return f.conversations, nil
}

func (f *fakeRepository) GetConversation(_ context.Context, _ string, conversationID string) (*ConversationDetail, error) {
	detail, ok := f.details[conversationID]
	if !ok {
		return nil, ErrNotFound
	}
	return detail, nil
}

func (f *fakeRepository) StoreInboundEvent(_ context.Context, event InboundEvent) (*StoredEventResult, error) {
	if existing, ok := f.stored[event.IDempotencyKey]; ok {
		next := *existing
		next.Created = false
		return &next, nil
	}
	detail := &ConversationDetail{
		ConversationSummary: ConversationSummary{ID: "conv_1", OrgID: event.OrgID, Title: event.Subject},
		Messages: []Message{{
			ID:             "msg_1",
			OrgID:          event.OrgID,
			ConversationID: "conv_1",
			Direction:      event.Direction,
			BodyText:       event.BodyText,
			OccurredAt:     event.OccurredAt,
			CreatedAt:      event.OccurredAt,
		}},
	}
	result := &StoredEventResult{Detail: detail, Message: &detail.Messages[0], Created: true}
	f.stored[event.IDempotencyKey] = result
	return result, nil
}

func (f *fakeRepository) AddMessage(_ context.Context, input AddMessageInput) (*Message, error) {
	f.lastMessage = input
	return &Message{
		ID:             "msg_reply",
		OrgID:          input.OrgID,
		ConversationID: input.ConversationID,
		Direction:      input.Direction,
		BodyText:       input.BodyText,
		Internal:       input.Internal,
		OccurredAt:     input.OccurredAt,
		CreatedAt:      input.OccurredAt,
	}, nil
}

func (f *fakeRepository) UpdateStatus(_ context.Context, input StatusUpdate) (*ConversationDetail, error) {
	f.statusUpdate = input
	return f.details[input.ConversationID], nil
}

func (f *fakeRepository) UpdateAssignment(_ context.Context, input AssignmentUpdate) (*ConversationDetail, error) {
	return f.details[input.ConversationID], nil
}

func (f *fakeRepository) AddTag(_ context.Context, orgID, conversationID, tag string) (*ConversationDetail, error) {
	detail := f.details[conversationID]
	detail.Tags = append(detail.Tags, tag)
	return detail, nil
}

func (f *fakeRepository) RemoveTag(_ context.Context, _ string, conversationID, tag string) (*ConversationDetail, error) {
	detail := f.details[conversationID]
	next := detail.Tags[:0]
	for _, current := range detail.Tags {
		if current != tag {
			next = append(next, current)
		}
	}
	detail.Tags = next
	return detail, nil
}

func (f *fakeRepository) ReviewAIAction(_ context.Context, _ AIActionReview) error {
	return nil
}

type fakePublisher struct {
	subjects []string
}

func (f *fakePublisher) Publish(_ context.Context, subject string, _ any) error {
	f.subjects = append(f.subjects, subject)
	return nil
}

func TestIngestEventNormalizesAndPublishesOnce(t *testing.T) {
	repository := newFakeRepository()
	publisher := &fakePublisher{}
	now := time.Date(2026, time.June, 3, 12, 0, 0, 0, time.UTC)
	service := NewService(repository, publisher, WithNow(func() time.Time { return now }))

	result, err := service.IngestEvent(context.Background(), InboundEvent{
		OrgID:             "org_1",
		Provider:          "Email",
		ProviderEventID:   "evt_1",
		ProviderMessageID: "m_1",
		ProviderThreadID:  "thread_1",
		Subject:           "Need help",
		From:              ParticipantInput{Name: "Ada", Email: "ADA@example.com"},
		BodyText:          "Hello",
	})
	if err != nil {
		t.Fatalf("IngestEvent() error = %v", err)
	}
	if result.Detail.ID != "conv_1" {
		t.Fatalf("conversation id = %q, want conv_1", result.Detail.ID)
	}
	if len(publisher.subjects) != 1 || publisher.subjects[0] != SubjectMessageReceived {
		t.Fatalf("subjects = %#v, want message.received once", publisher.subjects)
	}

	result, err = service.IngestEvent(context.Background(), InboundEvent{
		OrgID:             "org_1",
		Provider:          "email",
		ProviderEventID:   "evt_1",
		ProviderMessageID: "m_1",
		ProviderThreadID:  "thread_1",
		Subject:           "Need help",
		From:              ParticipantInput{Name: "Ada", Email: "ada@example.com"},
		BodyText:          "Hello",
	})
	if err != nil {
		t.Fatalf("second IngestEvent() error = %v", err)
	}
	if result.Created {
		t.Fatal("second result Created = true, want false")
	}
	if len(publisher.subjects) != 1 {
		t.Fatalf("subjects after duplicate = %#v, want unchanged", publisher.subjects)
	}
}

func TestIngestEventRejectsMissingBody(t *testing.T) {
	service := NewService(newFakeRepository(), nil)
	_, err := service.IngestEvent(context.Background(), InboundEvent{
		OrgID:    "org_1",
		Subject:  "Need help",
		From:     ParticipantInput{Email: "ada@example.com"},
		BodyText: "",
	})
	if !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("error = %v, want ErrInvalidInput", err)
	}
}

func TestAddMessageDefaultsOutboundAndPublishes(t *testing.T) {
	repository := newFakeRepository()
	publisher := &fakePublisher{}
	now := time.Date(2026, time.June, 3, 12, 10, 0, 0, time.UTC)
	service := NewService(repository, publisher, WithNow(func() time.Time { return now }))

	message, err := service.AddMessage(context.Background(), AddMessageInput{
		OrgID:          "org_1",
		ConversationID: "conv_1",
		ActorUserID:    "user_1",
		BodyText:       "Reply",
	})
	if err != nil {
		t.Fatalf("AddMessage() error = %v", err)
	}
	if message.Direction != DirectionOutbound {
		t.Fatalf("direction = %q, want outbound", message.Direction)
	}
	if repository.lastMessage.OccurredAt != now {
		t.Fatalf("occurred_at = %v, want %v", repository.lastMessage.OccurredAt, now)
	}
	if len(publisher.subjects) != 1 || publisher.subjects[0] != SubjectMessageSent {
		t.Fatalf("subjects = %#v, want message.sent", publisher.subjects)
	}
}

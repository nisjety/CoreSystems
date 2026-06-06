package conversation

import (
	"context"
	"errors"
	"time"
)

const (
	StatusOpen    = "open"
	StatusPending = "pending"
	StatusSolved  = "solved"
	StatusClosed  = "closed"

	DirectionInbound  = "inbound"
	DirectionOutbound = "outbound"

	SubjectConversationCreated = "velion.application.conversation.created"
	SubjectConversationUpdated = "velion.application.conversation.updated"
	SubjectMessageReceived     = "velion.application.conversation.message.received"
	SubjectMessageSent         = "velion.application.conversation.message.sent"
	SubjectNoteCreated         = "velion.application.conversation.note.created"
	SubjectStatusChanged       = "velion.application.conversation.status.changed"
	SubjectAssignmentChanged   = "velion.application.conversation.assignment.changed"
	SubjectTagAdded            = "velion.application.conversation.tag.added"
	SubjectTagRemoved          = "velion.application.conversation.tag.removed"
	SubjectAIActionReviewed    = "velion.application.conversation.ai_action.reviewed"
)

var (
	ErrNotFound       = errors.New("conversation not found")
	ErrInvalidInput   = errors.New("invalid input")
	ErrForbidden      = errors.New("forbidden")
	ErrAlreadyHandled = errors.New("event already handled")
)

type EventPublisher interface {
	Publish(ctx context.Context, subject string, payload any) error
}

type Repository interface {
	ListInboxes(ctx context.Context, orgID string) ([]Inbox, error)
	ListConversations(ctx context.Context, filter ListFilter) ([]ConversationSummary, error)
	GetConversation(ctx context.Context, orgID, conversationID string) (*ConversationDetail, error)
	StoreInboundEvent(ctx context.Context, event InboundEvent) (*StoredEventResult, error)
	AddMessage(ctx context.Context, input AddMessageInput) (*Message, error)
	UpdateStatus(ctx context.Context, input StatusUpdate) (*ConversationDetail, error)
	UpdateAssignment(ctx context.Context, input AssignmentUpdate) (*ConversationDetail, error)
	AddTag(ctx context.Context, orgID, conversationID, tag string) (*ConversationDetail, error)
	RemoveTag(ctx context.Context, orgID, conversationID, tag string) (*ConversationDetail, error)
	ReviewAIAction(ctx context.Context, input AIActionReview) error
}

type Inbox struct {
	ID        string    `json:"id"`
	OrgID     string    `json:"org_id"`
	Name      string    `json:"name"`
	Channel   string    `json:"channel"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

type Contact struct {
	ID    string `json:"id"`
	Name  string `json:"name"`
	Email string `json:"email"`
	Phone string `json:"phone,omitempty"`
}

type ConversationSummary struct {
	ID                 string     `json:"id"`
	OrgID              string     `json:"org_id"`
	InboxID            string     `json:"inbox_id"`
	Title              string     `json:"title"`
	Status             string     `json:"status"`
	Priority           string     `json:"priority"`
	Channel            string     `json:"channel"`
	Provider           string     `json:"provider,omitempty"`
	ProviderThreadID   string     `json:"provider_thread_id,omitempty"`
	AssigneeUserID     string     `json:"assignee_user_id,omitempty"`
	AssigneeName       string     `json:"assignee_name,omitempty"`
	LastMessagePreview string     `json:"last_message_preview,omitempty"`
	LastMessageAt      *time.Time `json:"last_message_at,omitempty"`
	Contact            Contact    `json:"contact"`
	Tags               []string   `json:"tags"`
	CreatedAt          time.Time  `json:"created_at"`
	UpdatedAt          time.Time  `json:"updated_at"`
}

type ConversationDetail struct {
	ConversationSummary
	Messages []Message `json:"messages"`
}

type Message struct {
	ID                string    `json:"id"`
	OrgID             string    `json:"org_id"`
	ConversationID    string    `json:"conversation_id"`
	Direction         string    `json:"direction"`
	SenderType        string    `json:"sender_type"`
	SenderName        string    `json:"sender_name,omitempty"`
	SenderEmail       string    `json:"sender_email,omitempty"`
	BodyText          string    `json:"body_text"`
	BodyHTML          string    `json:"body_html,omitempty"`
	Internal          bool      `json:"internal"`
	Provider          string    `json:"provider,omitempty"`
	ProviderMessageID string    `json:"provider_message_id,omitempty"`
	ProviderEventID   string    `json:"provider_event_id,omitempty"`
	OccurredAt        time.Time `json:"occurred_at"`
	CreatedAt         time.Time `json:"created_at"`
}

type ParticipantInput struct {
	Name  string `json:"name"`
	Email string `json:"email"`
}

type AttachmentInput struct {
	Filename    string `json:"filename"`
	MimeType    string `json:"mime_type"`
	SizeBytes   int64  `json:"size_bytes"`
	StorageRef  string `json:"storage_ref"`
	ProviderRef string `json:"provider_ref"`
}

type InboundEvent struct {
	IDempotencyKey    string             `json:"idempotency_key"`
	OrgID             string             `json:"org_id"`
	ConnectionID      string             `json:"connection_id,omitempty"`
	Provider          string             `json:"provider"`
	ProviderEventID   string             `json:"provider_event_id,omitempty"`
	ProviderMessageID string             `json:"provider_message_id,omitempty"`
	ProviderThreadID  string             `json:"provider_thread_id,omitempty"`
	Direction         string             `json:"direction,omitempty"`
	Subject           string             `json:"subject"`
	From              ParticipantInput   `json:"from"`
	To                []ParticipantInput `json:"to,omitempty"`
	BodyText          string             `json:"body_text"`
	BodyHTML          string             `json:"body_html,omitempty"`
	Attachments       []AttachmentInput  `json:"attachments,omitempty"`
	OccurredAt        time.Time          `json:"occurred_at"`
}

type StoredEventResult struct {
	Detail  *ConversationDetail `json:"detail"`
	Message *Message            `json:"message"`
	Created bool                `json:"created"`
}

type AddMessageInput struct {
	OrgID          string
	ConversationID string
	ActorUserID    string
	ActorName      string
	ActorEmail     string
	BodyText       string
	BodyHTML       string
	Internal       bool
	Direction      string
	OccurredAt     time.Time
}

type ListFilter struct {
	OrgID         string
	InboxID       string
	Status        string
	Assigned      string
	Channel       string
	Query         string
	Limit         int
	CursorUpdated *time.Time
	CursorID      string
}

type StatusUpdate struct {
	OrgID          string
	ConversationID string
	Status         string
	ActorUserID    string
}

type AssignmentUpdate struct {
	OrgID          string
	ConversationID string
	AssigneeUserID string
	AssigneeName   string
	ActorUserID    string
}

type AIActionReview struct {
	OrgID      string
	AIActionID string
	ReviewerID string
	Decision   string
	Comment    string
	OccurredAt time.Time
}

type LifecycleEvent struct {
	ID             string         `json:"id"`
	Type           string         `json:"type"`
	OrgID          string         `json:"org_id"`
	ConversationID string         `json:"conversation_id,omitempty"`
	MessageID      string         `json:"message_id,omitempty"`
	ActorUserID    string         `json:"actor_user_id,omitempty"`
	Data           map[string]any `json:"data,omitempty"`
	OccurredAt     time.Time      `json:"occurred_at"`
}

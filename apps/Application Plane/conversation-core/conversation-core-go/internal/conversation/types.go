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
	SubjectAIActionExecuted    = "velion.application.conversation.ai_action.executed"
	// SubjectAIActionSendFailed is the terminal lifecycle event for an
	// outbound-send (draft.reply) that failed permanently — the UI must surface
	// this honestly rather than ever claiming "sent". It stays in the
	// application namespace, covered by the existing application stream.
	SubjectAIActionSendFailed = "velion.application.conversation.ai_action.send_failed"
	// SubjectModelActionProposed is the Model-Plane → Application-Plane subject a
	// model (or hook) publishes to propose an action (e.g. a draft.reply) into the
	// HITL review queue. It lives in the model namespace, so it needs its own
	// JetStream stream (see eventing.EnsureModelStream); the application stream
	// does NOT cover it.
	SubjectModelActionProposed = "velion.model.action.proposed"
	SubjectTicketSuggested     = "velion.application.conversation.ticket.suggested"
	SubjectTicketCreated       = "velion.application.conversation.ticket.created"
	SubjectTicketUpdated       = "velion.application.conversation.ticket.updated"
	SubjectTicketAssigned      = "velion.application.conversation.ticket.assigned"
	SubjectTicketLinked        = "velion.application.conversation.ticket.linked"
	SubjectTicketResolved      = "velion.application.conversation.ticket.resolved"
)

var (
	ErrNotFound       = errors.New("conversation not found")
	ErrInvalidInput   = errors.New("invalid input")
	ErrForbidden      = errors.New("forbidden")
	ErrAlreadyHandled = errors.New("event already handled")
	ErrConflict       = errors.New("conversation resource conflict")
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
	CreateAIAction(ctx context.Context, input CreateAIActionInput) (*AIAction, error)
	ListAIActions(ctx context.Context, filter AIActionListFilter) ([]AIAction, error)
	ListTickets(ctx context.Context, filter TicketListFilter) ([]Ticket, error)
	GetTicket(ctx context.Context, orgID, ticketID string) (*Ticket, error)
	GetTicketByConversation(ctx context.Context, orgID, conversationID string) (*Ticket, error)
	CreateTicket(ctx context.Context, input CreateTicketInput) (*Ticket, error)
	UpdateTicket(ctx context.Context, input UpdateTicketInput) (*Ticket, error)
	LinkTicketResource(ctx context.Context, input LinkTicketResourceInput) (*TicketLinkedResource, error)
	RecordTicketClassification(ctx context.Context, input TicketClassificationInput, payload map[string]any) (*TicketClassification, error)
	ListTicketViews(ctx context.Context, orgID string) ([]TicketView, error)
	CreateTicketView(ctx context.Context, input CreateTicketViewInput) (*TicketView, error)
	UpdateTicketView(ctx context.Context, input UpdateTicketViewInput) (*TicketView, error)
	ListTicketMacros(ctx context.Context, orgID string) ([]TicketMacro, error)
	GetTicketMacro(ctx context.Context, orgID, macroID string) (*TicketMacro, error)
	CreateTicketMacro(ctx context.Context, input CreateTicketMacroInput) (*TicketMacro, error)
	UpdateTicketMacro(ctx context.Context, input UpdateTicketMacroInput) (*TicketMacro, error)
	RecordTicketMacroRun(ctx context.Context, input TicketMacroRunInput) error
	ListTicketAutomationRules(ctx context.Context, orgID string) ([]TicketAutomationRule, error)
	CreateTicketAutomationRule(ctx context.Context, input CreateTicketAutomationRuleInput) (*TicketAutomationRule, error)
	UpdateTicketAutomationRule(ctx context.Context, input UpdateTicketAutomationRuleInput) (*TicketAutomationRule, error)
	ListSLAPolicies(ctx context.Context, orgID string) ([]SLAPolicy, error)
	CreateSLAPolicy(ctx context.Context, input CreateSLAPolicyInput) (*SLAPolicy, error)
	UpdateSLAPolicy(ctx context.Context, input UpdateSLAPolicyInput) (*SLAPolicy, error)
	CreateTicketChecklist(ctx context.Context, input CreateTicketChecklistInput) (*TicketChecklist, error)
	UpdateTicketChecklistItem(ctx context.Context, input UpdateTicketChecklistItemInput) (*TicketChecklist, error)
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

type Ticket struct {
	ID                  string                 `json:"id"`
	OrgID               string                 `json:"org_id"`
	ConversationID      string                 `json:"conversation_id"`
	TicketKey           string                 `json:"ticket_key"`
	Status              string                 `json:"status"`
	Priority            string                 `json:"priority"`
	Severity            string                 `json:"severity"`
	Category            string                 `json:"category,omitempty"`
	Intent              string                 `json:"intent,omitempty"`
	AssigneeUserID      string                 `json:"assignee_user_id,omitempty"`
	AssigneeName        string                 `json:"assignee_name,omitempty"`
	TeamID              string                 `json:"team_id,omitempty"`
	TeamName            string                 `json:"team_name,omitempty"`
	DueAt               *time.Time             `json:"due_at,omitempty"`
	Source              string                 `json:"source"`
	AIConfidence        float64                `json:"ai_confidence,omitempty"`
	AIReason            string                 `json:"ai_reason,omitempty"`
	CreatedBy           string                 `json:"created_by,omitempty"`
	WaitingSince        *time.Time             `json:"waiting_since,omitempty"`
	LastCustomerReplyAt *time.Time             `json:"last_customer_reply_at,omitempty"`
	FirstResponseAt     *time.Time             `json:"first_response_at,omitempty"`
	ResolvedAt          *time.Time             `json:"resolved_at,omitempty"`
	SnoozedUntil        *time.Time             `json:"snoozed_until,omitempty"`
	SLAPolicyID         string                 `json:"sla_policy_id,omitempty"`
	EscalationAt        *time.Time             `json:"escalation_at,omitempty"`
	Labels              []string               `json:"labels"`
	SLAState            string                 `json:"sla_state"`
	Conversation        *ConversationSummary   `json:"conversation,omitempty"`
	LinkedResources     []TicketLinkedResource `json:"linked_resources,omitempty"`
	Checklists          []TicketChecklist      `json:"checklists,omitempty"`
	CreatedAt           time.Time              `json:"created_at"`
	UpdatedAt           time.Time              `json:"updated_at"`
}

type TicketLinkedResource struct {
	ID              string         `json:"id"`
	OrgID           string         `json:"org_id"`
	TicketID        string         `json:"ticket_id"`
	ConversationID  string         `json:"conversation_id"`
	LinkType        string         `json:"link_type"`
	ResourceKind    string         `json:"resource_kind"`
	ResourceID      string         `json:"resource_id,omitempty"`
	ResourceURL     string         `json:"resource_url,omitempty"`
	Label           string         `json:"label,omitempty"`
	Metadata        map[string]any `json:"metadata,omitempty"`
	CreatedByUserID string         `json:"created_by_user_id,omitempty"`
	CreatedAt       time.Time      `json:"created_at"`
}

type TicketClassification struct {
	ID             string         `json:"id"`
	OrgID          string         `json:"org_id"`
	ConversationID string         `json:"conversation_id"`
	Outcome        string         `json:"outcome"`
	Confidence     float64        `json:"confidence"`
	Reason         string         `json:"reason"`
	Payload        map[string]any `json:"payload"`
	Ticket         *Ticket        `json:"ticket,omitempty"`
	CreatedAt      time.Time      `json:"created_at"`
}

type TicketView struct {
	ID           string         `json:"id"`
	OrgID        string         `json:"org_id"`
	Name         string         `json:"name"`
	Scope        string         `json:"scope"`
	OwnerUserID  string         `json:"owner_user_id,omitempty"`
	TeamID       string         `json:"team_id,omitempty"`
	Visibility   string         `json:"visibility"`
	Filter       map[string]any `json:"filter"`
	Sort         map[string]any `json:"sort"`
	GroupBy      string         `json:"group_by,omitempty"`
	SidebarOrder int            `json:"sidebar_order"`
	CreatedAt    time.Time      `json:"created_at"`
	UpdatedAt    time.Time      `json:"updated_at"`
}

type TicketMacro struct {
	ID          string         `json:"id"`
	OrgID       string         `json:"org_id"`
	Name        string         `json:"name"`
	Description string         `json:"description,omitempty"`
	Visibility  string         `json:"visibility"`
	TeamID      string         `json:"team_id,omitempty"`
	Active      bool           `json:"active"`
	Actions     map[string]any `json:"actions"`
	Conditions  map[string]any `json:"conditions"`
	CreatedAt   time.Time      `json:"created_at"`
	UpdatedAt   time.Time      `json:"updated_at"`
}

type TicketAutomationRule struct {
	ID         string         `json:"id"`
	OrgID      string         `json:"org_id"`
	Name       string         `json:"name"`
	EventName  string         `json:"event_name"`
	Active     bool           `json:"active"`
	Conditions map[string]any `json:"conditions"`
	Actions    map[string]any `json:"actions"`
	CreatedAt  time.Time      `json:"created_at"`
	UpdatedAt  time.Time      `json:"updated_at"`
}

type SLAPolicy struct {
	ID                   string         `json:"id"`
	OrgID                string         `json:"org_id"`
	Name                 string         `json:"name"`
	Active               bool           `json:"active"`
	Conditions           map[string]any `json:"conditions"`
	CalendarRef          string         `json:"calendar_ref,omitempty"`
	FirstResponseMinutes int            `json:"first_response_minutes"`
	NextResponseMinutes  int            `json:"next_response_minutes"`
	ResolutionMinutes    int            `json:"resolution_minutes"`
	CreatedAt            time.Time      `json:"created_at"`
	UpdatedAt            time.Time      `json:"updated_at"`
}

type TicketChecklist struct {
	ID              string                `json:"id"`
	OrgID           string                `json:"org_id"`
	TicketID        string                `json:"ticket_id"`
	Name            string                `json:"name"`
	TemplateID      string                `json:"template_id,omitempty"`
	CreatedByUserID string                `json:"created_by_user_id,omitempty"`
	Items           []TicketChecklistItem `json:"items"`
	CreatedAt       time.Time             `json:"created_at"`
	UpdatedAt       time.Time             `json:"updated_at"`
}

type TicketChecklistItem struct {
	ID          string    `json:"id"`
	OrgID       string    `json:"org_id"`
	ChecklistID string    `json:"checklist_id"`
	Label       string    `json:"label"`
	Completed   bool      `json:"completed"`
	Position    int       `json:"position"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
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
	Name string `json:"name"`
	// Email and Phone identify the participant across channels — email-based
	// channels (email/microsoft/google) populate Email; WhatsApp populates
	// Phone. Contact resolution in StoreInboundEvent prefers Email, falls
	// back to Phone, and last to a provider-scoped reference (e.g. a
	// Messenger PSID) so no two channels' contacts collide.
	Email string `json:"email"`
	Phone string `json:"phone,omitempty"`
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

type TicketListFilter struct {
	OrgID    string
	Queue    string
	Status   string
	Assigned string
	TeamID   string
	Label    string
	Priority string
	Severity string
	SLAState string
	Query    string
	Limit    int
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

// AIAction is a model-proposed action awaiting (or having received) a human
// review decision — the read shape backing the HITL review queue. It mirrors a
// row of conversation_ai_actions.
type AIAction struct {
	ID             string         `json:"id"`
	OrgID          string         `json:"org_id"`
	ConversationID string         `json:"conversation_id"`
	Kind           string         `json:"kind"`
	Status         string         `json:"status"`
	Payload        map[string]any `json:"payload"`
	CreatedBy      string         `json:"created_by"`
	ReviewedBy     string         `json:"reviewed_by,omitempty"`
	ReviewedAt     *time.Time     `json:"reviewed_at,omitempty"`
	CreatedAt      time.Time      `json:"created_at"`
	UpdatedAt      time.Time      `json:"updated_at"`
}

type AIActionListFilter struct {
	OrgID          string
	Status         string
	ConversationID string
	Limit          int
}

// CreateAIActionInput queues a model-proposed action (e.g. a draft.reply) into
// the HITL review queue at status 'suggested'. It is the generic path used by
// both the new POST /ai-actions route and the model-proposed consumer, so a
// human or hook can propose an action end-to-end.
type CreateAIActionInput struct {
	OrgID          string
	ConversationID string
	Kind           string
	Payload        map[string]any
	CreatedBy      string
}

// ChannelThreadRef is the per-conversation outbound send target resolved from
// conversation_channel_thread_refs: which provider/connection/thread a reply is
// addressed to. It is the source the draft.reply executor sends through.
type ChannelThreadRef struct {
	OrgID            string `json:"org_id"`
	ConversationID   string `json:"conversation_id"`
	Provider         string `json:"provider"`
	ConnectionID     string `json:"connection_id"`
	ProviderThreadID string `json:"provider_thread_id"`
}

type CreateTicketInput struct {
	OrgID               string
	ConversationID      string
	Status              string
	Priority            string
	Severity            string
	Category            string
	Intent              string
	AssigneeUserID      string
	AssigneeName        string
	TeamID              string
	TeamName            string
	DueAt               *time.Time
	Source              string
	AIConfidence        float64
	AIReason            string
	CreatedBy           string
	WaitingSince        *time.Time
	LastCustomerReplyAt *time.Time
	FirstResponseAt     *time.Time
	ResolvedAt          *time.Time
	SnoozedUntil        *time.Time
	SLAPolicyID         string
	EscalationAt        *time.Time
	Labels              []string
	ActorUserID         string
}

type UpdateTicketInput struct {
	OrgID               string
	TicketID            string
	Status              *string
	Priority            *string
	Severity            *string
	Category            *string
	Intent              *string
	AssigneeUserID      *string
	AssigneeName        *string
	TeamID              *string
	TeamName            *string
	DueAt               *time.Time
	Source              *string
	AIConfidence        *float64
	AIReason            *string
	WaitingSince        *time.Time
	LastCustomerReplyAt *time.Time
	FirstResponseAt     *time.Time
	ResolvedAt          *time.Time
	SnoozedUntil        *time.Time
	SLAPolicyID         *string
	EscalationAt        *time.Time
	Labels              *[]string
	ActorUserID         string
}

type LinkTicketResourceInput struct {
	OrgID           string
	TicketID        string
	LinkType        string
	ResourceKind    string
	ResourceID      string
	ResourceURL     string
	Label           string
	Metadata        map[string]any
	CreatedByUserID string
}

type TicketClassificationInput struct {
	OrgID              string
	ConversationID     string
	Outcome            string
	Confidence         float64
	Reason             string
	SuggestedFields    map[string]any
	EvidenceMessageIDs []string
	ActorUserID        string
}

type CreateTicketViewInput struct {
	OrgID        string
	Name         string
	Scope        string
	OwnerUserID  string
	TeamID       string
	Visibility   string
	Filter       map[string]any
	Sort         map[string]any
	GroupBy      string
	SidebarOrder int
	ActorUserID  string
}

type UpdateTicketViewInput struct {
	OrgID        string
	ID           string
	Name         *string
	Scope        *string
	OwnerUserID  *string
	TeamID       *string
	Visibility   *string
	Filter       *map[string]any
	Sort         *map[string]any
	GroupBy      *string
	SidebarOrder *int
	ActorUserID  string
}

type CreateTicketMacroInput struct {
	OrgID       string
	Name        string
	Description string
	Visibility  string
	TeamID      string
	Active      bool
	Actions     map[string]any
	Conditions  map[string]any
	ActorUserID string
}

type UpdateTicketMacroInput struct {
	OrgID       string
	ID          string
	Name        *string
	Description *string
	Visibility  *string
	TeamID      *string
	Active      *bool
	Actions     *map[string]any
	Conditions  *map[string]any
	ActorUserID string
}

type TicketMacroRunInput struct {
	OrgID       string
	TicketID    string
	MacroID     string
	ActorUserID string
	Actions     map[string]any
}

type TicketMacroRunResult struct {
	Ticket *Ticket     `json:"ticket"`
	Macro  TicketMacro `json:"macro"`
}

type CreateTicketAutomationRuleInput struct {
	OrgID       string
	Name        string
	EventName   string
	Active      bool
	Conditions  map[string]any
	Actions     map[string]any
	ActorUserID string
}

type UpdateTicketAutomationRuleInput struct {
	OrgID       string
	ID          string
	Name        *string
	EventName   *string
	Active      *bool
	Conditions  *map[string]any
	Actions     *map[string]any
	ActorUserID string
}

type CreateSLAPolicyInput struct {
	OrgID                string
	Name                 string
	Active               bool
	Conditions           map[string]any
	CalendarRef          string
	FirstResponseMinutes int
	NextResponseMinutes  int
	ResolutionMinutes    int
	ActorUserID          string
}

type UpdateSLAPolicyInput struct {
	OrgID                string
	ID                   string
	Name                 *string
	Active               *bool
	Conditions           *map[string]any
	CalendarRef          *string
	FirstResponseMinutes *int
	NextResponseMinutes  *int
	ResolutionMinutes    *int
	ActorUserID          string
}

type CreateTicketChecklistInput struct {
	OrgID           string
	TicketID        string
	Name            string
	TemplateID      string
	Items           []string
	CreatedByUserID string
}

type UpdateTicketChecklistItemInput struct {
	OrgID       string
	TicketID    string
	ChecklistID string
	ItemID      string
	Completed   bool
	ActorUserID string
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

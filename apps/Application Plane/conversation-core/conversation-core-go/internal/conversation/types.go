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

	TicketSideConversationOpen   = "open"
	TicketSideConversationClosed = "closed"

	// FeedbackProvider marks the synthetic inbound channel used by
	// Service.SubmitFeedback -- a signed-in org member reporting friction from
	// inside the product, not a real external provider. FeedbackTag is applied
	// to every conversation it creates (in the submitter's own org, and again
	// in the mirrored copy -- see Service.mirrorFeedback) so the Inbox can
	// filter pilot feedback into its own queue instead of mixing it with
	// customer traffic.
	FeedbackProvider = "pilot-feedback"
	FeedbackTag      = "pilot-feedback"
	// FeedbackMirrorTag is applied ONLY to the mirrored copy of a feedback
	// conversation (Service.mirrorFeedback), never to the submitter's own-org
	// original. It lets a team member distinguish "feedback filed directly in
	// our own org" from "feedback mirrored in from an external pilot org" at a
	// glance, without having to read the body.
	FeedbackMirrorTag = "cross-org-feedback"

	SubjectConversationCreated = "verevon.application.conversation.created"
	SubjectConversationUpdated = "verevon.application.conversation.updated"
	SubjectMessageReceived     = "verevon.application.conversation.message.received"
	SubjectMessageSent         = "verevon.application.conversation.message.sent"
	SubjectNoteCreated         = "verevon.application.conversation.note.created"
	SubjectStatusChanged       = "verevon.application.conversation.status.changed"
	SubjectAssignmentChanged   = "verevon.application.conversation.assignment.changed"
	SubjectTagAdded            = "verevon.application.conversation.tag.added"
	SubjectTagRemoved          = "verevon.application.conversation.tag.removed"
	SubjectAIActionReviewed    = "verevon.application.conversation.ai_action.reviewed"
	SubjectAIActionExecuted    = "verevon.application.conversation.ai_action.executed"
	// SubjectAIActionSendFailed is the terminal lifecycle event for an
	// outbound-send (draft.reply) that failed permanently — the UI must surface
	// this honestly rather than ever claiming "sent". It stays in the
	// application namespace, covered by the existing application stream.
	SubjectAIActionSendFailed = "verevon.application.conversation.ai_action.send_failed"
	// SubjectAIActionSendUnknown is emitted when a provider may have accepted a
	// reply but the result cannot be proven. It is reconciliation-required and
	// must never trigger an automatic retransmission.
	SubjectAIActionSendUnknown = "verevon.application.conversation.ai_action.send_unknown"
	// SubjectModelActionProposed is the Model-Plane → Application-Plane subject a
	// model (or hook) publishes to propose an action (e.g. a draft.reply) into the
	// HITL review queue. It lives in the model namespace, so it needs its own
	// JetStream stream (see eventing.EnsureModelStream); the application stream
	// does NOT cover it.
	SubjectModelActionProposed         = "verevon.model.action.proposed"
	SubjectTicketSuggested             = "verevon.application.conversation.ticket.suggested"
	SubjectTicketCreated               = "verevon.application.conversation.ticket.created"
	SubjectTicketCreatedAudit          = "verevon.audit.v2.application.conversation-core.ticket_created"
	SubjectTicketUpdated               = "verevon.application.conversation.ticket.updated"
	SubjectTicketAssigned              = "verevon.application.conversation.ticket.assigned"
	SubjectTicketLinked                = "verevon.application.conversation.ticket.linked"
	SubjectTicketResolved              = "verevon.application.conversation.ticket.resolved"
	SubjectTicketAutomationRuleCreated = "verevon.application.conversation.ticket_automation_rule.created"
	SubjectTicketAutomationRuleUpdated = "verevon.application.conversation.ticket_automation_rule.updated"
)

var (
	ErrNotFound     = errors.New("conversation not found")
	ErrInvalidInput = errors.New("invalid input")
	ErrForbidden    = errors.New("forbidden")
	// Policy denials are returned by the durable Conversation Core boundary,
	// not inferred from gateway-only preflight checks. They protect every AI
	// proposal producer, including asynchronous Model Plane consumers.
	ErrPolicyUnavailable      = errors.New("support policy unavailable")
	ErrZDRAIProposalForbidden = errors.New("ai proposals forbidden by zero data retention")
	ErrAIReviewModeRequired   = errors.New("support ai review mode required")
	ErrAlreadyHandled         = errors.New("event already handled")
	ErrConflict               = errors.New("conversation resource conflict")
	// ErrSendFailed is returned when a human agent's reply to a channel-backed
	// conversation was attempted but could not be delivered to the customer (the
	// integration-corev2 send errored). The HTTP layer surfaces it as 502 so the
	// Inbox never shows a phantom "Reply sent" for a message the customer never
	// received, and no outbound message row is persisted for it.
	ErrSendFailed          = errors.New("outbound reply delivery failed")
	ErrDeliveryUnavailable = errors.New("outbound reply delivery is unavailable")
	// ErrDeliveryUnknown means the provider may have accepted an outbound
	// message but conversation-core cannot prove the final outcome. Callers must
	// surface reconciliation-required state and must not blindly retry it.
	ErrDeliveryUnknown = errors.New("outbound reply delivery outcome is unknown")
)

const (
	OutboundIntentSending   = "sending"
	OutboundIntentRetryable = "retryable"
	OutboundIntentSubmitted = "submitted"
	OutboundIntentFailed    = "failed"
	OutboundIntentUnknown   = "unknown"

	// Provider delivery evidence is distinct from the durable outbound-intent
	// status above. `submitted` means a provider accepted a send request;
	// delivery/read evidence can only be set later by a matching provider
	// callback carrying the exact provider message identifier.
	ProviderDeliveryUnconfirmed = "unconfirmed"
	ProviderDeliveryDelivered   = "delivered"
	ProviderDeliveryRead        = "read"
	ProviderDeliveryFailed      = "failed"
)

// OutboundIntentErrorStaleSendingTimeout is the error_code
// ReconcileStaleOutboundIntents stamps on a `sending` row it flips to
// `unknown` because it sat unresolved past the configured timeout (a crash or
// lost process between ClaimOutboundIntent and
// FinalizeOutboundIntent/MarkOutboundIntentOutcome). It is intentionally
// distinct from the send-path error codes so operators can tell "we never
// heard back" apart from a provider-reported failure.
const OutboundIntentErrorStaleSendingTimeout = "stale_sending_timeout"

type EventPublisher interface {
	Publish(ctx context.Context, subject string, payload any) error
}

// TicketOperationOutboxEvent contains the bounded, content-free event used to
// project a durable ticket operation. It is claimed by a lease before publish
// and acknowledged only after the broker accepts its stable event ID.
type TicketOperationOutboxEvent struct {
	ID             string
	OrgID          string
	ConversationID string
	ActorUserID    string
	Payload        map[string]any
	CreatedAt      time.Time
}

// AuditObservation is the content-minimized, stable-id event accepted by the
// Control Audit stream. The Conversation owner produces it only from a
// committed owner receipt; `Details` contains correlation identifiers rather
// than ticket content.
type AuditObservation struct {
	ID         string         `json:"event_id"`
	OccurredAt time.Time      `json:"occurred_at"`
	OrgID      string         `json:"org_id"`
	UserID     string         `json:"user_id,omitempty"`
	Plane      string         `json:"plane"`
	Producer   string         `json:"producer"`
	Event      string         `json:"event"`
	Subject    string         `json:"subject,omitempty"`
	ResourceID string         `json:"resource_id,omitempty"`
	Outcome    string         `json:"outcome"`
	Details    map[string]any `json:"details,omitempty"`
}

func (event AuditObservation) EventID() string { return event.ID }

// TicketOperationOutboxStore is intentionally narrower than Repository so the
// delivery worker cannot create or modify support records.
type TicketOperationOutboxStore interface {
	ClaimTicketOperationOutbox(ctx context.Context, workerID string, now time.Time, lease time.Duration, limit int) ([]TicketOperationOutboxEvent, error)
	AcknowledgeTicketOperationOutbox(ctx context.Context, eventID, workerID string, now time.Time) error
	ReleaseTicketOperationOutbox(ctx context.Context, eventID, workerID, reason string, retryAt time.Time) error
}

type Repository interface {
	ListInboxes(ctx context.Context, orgID string) ([]Inbox, error)
	ListConversations(ctx context.Context, filter ListFilter) ([]ConversationSummary, error)
	GetConversation(ctx context.Context, orgID, conversationID string) (*ConversationDetail, error)
	GetDraftLease(ctx context.Context, orgID, conversationID string) (*DraftLease, error)
	ClaimDraftLease(ctx context.Context, input DraftLeaseClaimInput) (*DraftLease, error)
	ReleaseDraftLease(ctx context.Context, orgID, conversationID, userID string) error
	GetConversationFollow(ctx context.Context, orgID, conversationID, userID string) (*ConversationFollow, error)
	FollowConversation(ctx context.Context, input ConversationFollowInput) (*ConversationFollow, error)
	UnfollowConversation(ctx context.Context, orgID, conversationID, userID string) error
	GetConversationCSATPreference(ctx context.Context, orgID, conversationID string) (*CSATPreference, error)
	SetConversationCSATPreference(ctx context.Context, input CSATPreferenceInput) (*CSATPreference, error)
	GetTicketCSATOutcome(ctx context.Context, orgID, ticketID string) (*TicketCSATOutcome, error)
	UpsertTicketCSATOutcome(ctx context.Context, input TicketCSATOutcomeInput) (*TicketCSATOutcome, error)
	GetCSATScorecard(ctx context.Context, orgID string) (*CSATScorecard, error)
	GetConversationDraft(ctx context.Context, orgID, conversationID, userID string) (*ConversationDraft, error)
	UpsertConversationDraft(ctx context.Context, input ConversationDraftInput) (*ConversationDraft, error)
	DeleteConversationDraft(ctx context.Context, orgID, conversationID, userID string) error
	StoreInboundEvent(ctx context.Context, event InboundEvent) (*StoredEventResult, error)
	AddMessage(ctx context.Context, input AddMessageInput) (*Message, error)
	ClaimOutboundIntent(ctx context.Context, input OutboundIntentClaimInput) (*OutboundIntentClaim, error)
	ListOutboundIntents(ctx context.Context, orgID, conversationID string) ([]OutboundIntent, error)
	ListOrganizationOutboundIntents(ctx context.Context, filter OutboundIntentListFilter) ([]OutboundIntent, error)
	FinalizeOutboundIntent(ctx context.Context, input OutboundIntentFinalizeInput) (*Message, error)
	MarkOutboundIntentOutcome(ctx context.Context, input OutboundIntentOutcomeInput) error
	RecordProviderDeliveryReceipt(ctx context.Context, input ProviderDeliveryReceiptInput) (bool, error)
	// ReconcileStaleOutboundIntents atomically flips every outbound intent
	// still `sending` after staleAfter to `unknown` and returns the reconciled
	// rows. It is the stuck-send sweep for a claim that never reached
	// FinalizeOutboundIntent or MarkOutboundIntentOutcome (process crash,
	// deploy, OOM): without it such a row stays `sending` forever, silently
	// blocking its idempotency key from ever being retried or reconciled. It
	// never marks a row `retryable`, because a `sending` row does not prove
	// the provider was never called — an automatic retry could double-send.
	ReconcileStaleOutboundIntents(ctx context.Context, staleAfter time.Duration) ([]OutboundIntent, error)
	GetMessage(ctx context.Context, orgID, messageID string) (*Message, error)
	// GetChannelThreadRefByConversation resolves the outbound send target
	// (provider/connection/thread) for a conversation, or ErrNotFound when the
	// conversation is not bound to an external channel. The Service uses it to
	// decide whether a human reply must be delivered through integration-corev2.
	GetChannelThreadRefByConversation(ctx context.Context, orgID, conversationID string) (*ChannelThreadRef, error)
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
	// CreateTicketOperation is the durable owner-plane operation boundary for
	// the generic human action surface. Unlike the legacy CreateTicket method,
	// it persists the exact idempotency/request binding, ticket, audit event,
	// and transactional-outbox event together before returning a receipt.
	CreateTicketOperation(ctx context.Context, input CreateTicketInput) (*TicketOperationReceipt, error)
	GetTicketOperation(ctx context.Context, orgID, actorUserID, idempotencyKey string) (*TicketOperationReceipt, error)
	UpdateTicket(ctx context.Context, input UpdateTicketInput) (*Ticket, error)
	LinkTicketResource(ctx context.Context, input LinkTicketResourceInput) (*TicketLinkedResource, error)
	RecordTicketClassification(ctx context.Context, input TicketClassificationInput, payload map[string]any) (*TicketClassification, error)
	ListTicketTeams(ctx context.Context, orgID string) ([]TicketTeam, error)
	GetTicketTeam(ctx context.Context, orgID, teamID string) (*TicketTeam, error)
	CreateTicketTeam(ctx context.Context, input CreateTicketTeamInput) (*TicketTeam, error)
	UpdateTicketTeam(ctx context.Context, input UpdateTicketTeamInput) (*TicketTeam, error)
	ListTicketViews(ctx context.Context, orgID string) ([]TicketView, error)
	CreateTicketView(ctx context.Context, input CreateTicketViewInput) (*TicketView, error)
	UpdateTicketView(ctx context.Context, input UpdateTicketViewInput) (*TicketView, error)
	ListTicketMacros(ctx context.Context, orgID string) ([]TicketMacro, error)
	GetTicketMacro(ctx context.Context, orgID, macroID string) (*TicketMacro, error)
	CreateTicketMacro(ctx context.Context, input CreateTicketMacroInput) (*TicketMacro, error)
	UpdateTicketMacro(ctx context.Context, input UpdateTicketMacroInput) (*TicketMacro, error)
	RecordTicketMacroRun(ctx context.Context, input TicketMacroRunInput) error
	RecordTicketChatHandoff(ctx context.Context, input TicketChatHandoffInput) (*Ticket, error)
	ListTicketAutomationRules(ctx context.Context, orgID string) ([]TicketAutomationRule, error)
	CreateTicketAutomationRule(ctx context.Context, input CreateTicketAutomationRuleInput) (*TicketAutomationRule, error)
	UpdateTicketAutomationRule(ctx context.Context, input UpdateTicketAutomationRuleInput) (*TicketAutomationRule, error)
	ListSLAPolicies(ctx context.Context, orgID string) ([]SLAPolicy, error)
	CreateSLAPolicy(ctx context.Context, input CreateSLAPolicyInput) (*SLAPolicy, error)
	UpdateSLAPolicy(ctx context.Context, input UpdateSLAPolicyInput) (*SLAPolicy, error)
	CreateTicketChecklist(ctx context.Context, input CreateTicketChecklistInput) (*TicketChecklist, error)
	UpdateTicketChecklistItem(ctx context.Context, input UpdateTicketChecklistItemInput) (*TicketChecklist, error)
	CreateTicketSideConversation(ctx context.Context, input CreateTicketSideConversationInput) (*TicketSideConversation, error)
	AddTicketSideConversationMessage(ctx context.Context, input AddTicketSideConversationMessageInput) (*TicketSideConversation, error)
	UpdateTicketSideConversation(ctx context.Context, input UpdateTicketSideConversationInput) (*TicketSideConversation, error)
	// HardPurgeByOrg permanently deletes every conversation_* row for orgID.
	// It is the conversation-core half of the cross-plane GDPR erasure fan-out
	// (see consumers.OrgErasureConsumer) and must remain safe to call more
	// than once for the same orgID (NATS at-least-once delivery).
	HardPurgeByOrg(ctx context.Context, orgID string) error
	// PurgeConversationDraftsByOrg deletes only the personal recovery drafts
	// held for one organization. It is intentionally narrower than
	// HardPurgeByOrg: enabling interactive Zero Data Retention must never
	// erase the durable support record, tickets, or provider evidence.
	PurgeConversationDraftsByOrg(ctx context.Context, orgID string) error

	// Support-recurrence corpus (semantic ticket-similarity preview). See
	// migrations/027_support_recurrence_corpus.sql.
	DistinctOrgIDsWithActiveTickets(ctx context.Context) ([]string, error)
	ActiveTicketsForSupportRecurrenceCorpus(ctx context.Context, orgID string) ([]Ticket, error)
	UpsertSupportRecurrenceCorpusEntry(ctx context.Context, orgID, ticketID string, embedding []float32, algorithmVersion string, corpusWindowStart time.Time) error
	EvictStaleSupportRecurrenceCorpusEntries(ctx context.Context, orgID string, windowStart time.Time) error
	PurgeSupportRecurrenceCorpusByOrg(ctx context.Context, orgID string) error
	ListSupportRecurrenceCorpus(ctx context.Context, orgID string) ([]SupportRecurrenceCorpusEntry, error)
}

// TicketActivityRepository is an optional read capability for the small,
// operator-facing ticket history. It deliberately exposes only the bounded
// audit fields that can be displayed in a work queue; raw audit payloads stay
// inside the service boundary.
type TicketActivityRepository interface {
	ListTicketActivity(ctx context.Context, orgID, ticketID string, limit int) ([]TicketActivity, error)
}

// ConversationActivityRepository exposes a deliberately narrow read model for
// the shared Inbox timeline. Raw audit payloads remain service-internal because
// they can contain provider or automation metadata that is not safe to render.
type ConversationActivityRepository interface {
	ListConversationActivity(ctx context.Context, orgID, conversationID string, limit int) ([]ConversationActivity, error)
}

// IncidentProblemRepository is intentionally a separate capability from the
// original ticket repository. It lets older repository test doubles remain
// useful while production must explicitly opt in before operational records
// can be created or changed.
type IncidentProblemRepository interface {
	ListIncidents(ctx context.Context, orgID string) ([]Incident, error)
	GetIncident(ctx context.Context, orgID, incidentID string) (*Incident, error)
	CreateIncident(ctx context.Context, input CreateIncidentInput) (*Incident, error)
	UpdateIncident(ctx context.Context, input UpdateIncidentInput) (*Incident, error)
	LinkIncidentTicket(ctx context.Context, input LinkIncidentTicketInput) (*IncidentTicketLink, error)
	ListProblems(ctx context.Context, orgID string) ([]Problem, error)
	GetProblem(ctx context.Context, orgID, problemID string) (*Problem, error)
	CreateProblem(ctx context.Context, input CreateProblemInput) (*Problem, error)
	UpdateProblem(ctx context.Context, input UpdateProblemInput) (*Problem, error)
}

// ApprovedIncidentActionRepository owns the atomic durable effect of a
// reviewed AI incident proposal. It is separate from ordinary Incident CRUD so
// an approval retry can be idempotent by AI-action ID.
type ApprovedIncidentActionRepository interface {
	CreateIncidentForApprovedAction(ctx context.Context, input ApprovedIncidentCreateInput) (*Incident, error)
}

// ApprovedProblemActionRepository owns the idempotent durable effect of a
// reviewed AI root-cause candidate. It never changes an Incident or Ticket;
// a human may associate the independently-created Problem later.
type ApprovedProblemActionRepository interface {
	CreateProblemForApprovedAction(ctx context.Context, input ApprovedProblemCreateInput) (*Problem, error)
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

// DraftLease is short-lived collision protection for a human composing a reply.
// It is never a lifecycle or assignment mutation.
type DraftLease struct {
	OrgID          string    `json:"org_id"`
	ConversationID string    `json:"conversation_id"`
	UserID         string    `json:"user_id"`
	ExpiresAt      time.Time `json:"expires_at"`
	UpdatedAt      time.Time `json:"updated_at"`
}

type DraftLeaseClaimInput struct {
	OrgID          string
	ConversationID string
	UserID         string
	ExpiresAt      time.Time
}

// ConversationFollow is one operator's durable preference to follow a
// conversation. It deliberately stores no customer content; Application Plane
// notification delivery remains separately membership- and preference-gated.
type ConversationFollow struct {
	OrgID          string    `json:"org_id"`
	ConversationID string    `json:"conversation_id"`
	UserID         string    `json:"user_id"`
	CreatedAt      time.Time `json:"created_at"`
}

type ConversationFollowInput struct {
	OrgID          string
	ConversationID string
	UserID         string
}

// CSATPreference is an explicit support-contact preference, not a Control
// Plane user-consent record. It is the prerequisite for any future survey
// invitation; it does not itself send a survey or expose contact data.
type CSATPreference struct {
	OrgID          string     `json:"org_id"`
	ConversationID string     `json:"conversation_id"`
	ContactID      string     `json:"contact_id"`
	OptedIn        bool       `json:"opted_in"`
	UpdatedBy      string     `json:"updated_by,omitempty"`
	UpdatedAt      *time.Time `json:"updated_at,omitempty"`
}

type CSATPreferenceInput struct {
	OrgID          string
	ConversationID string
	ActorUserID    string
	OptedIn        bool
}

// TicketCSATOutcome is a customer rating that an operator has recorded from a
// consented, resolved support case. It deliberately does not imply that a
// survey was sent: outbound survey delivery needs separate deployment
// authority and delivery evidence.
type TicketCSATOutcome struct {
	OrgID          string     `json:"org_id"`
	TicketID       string     `json:"ticket_id"`
	ConversationID string     `json:"conversation_id"`
	Score          int        `json:"score"`
	RecordedBy     string     `json:"recorded_by,omitempty"`
	RecordedAt     *time.Time `json:"recorded_at,omitempty"`
}

type TicketCSATOutcomeInput struct {
	OrgID      string
	TicketID   string
	Score      int
	RecordedBy string
}

// CSATScorecard reports only ratings actually recorded by customers. It has no
// response-rate denominator because Verevon does not currently dispatch surveys
// automatically; displaying a synthetic rate would be misleading.
type CSATScorecard struct {
	RatedTickets    int      `json:"rated_tickets"`
	PositiveRatings int      `json:"positive_ratings"`
	AverageScore    *float64 `json:"average_score,omitempty"`
	PositiveRate    *float64 `json:"positive_rate,omitempty"`
}

// ConversationDraft is a private recovery record for one operator's unfinished
// reply. It is not a shared note, timeline event, or AI proposal.
type ConversationDraft struct {
	OrgID          string    `json:"org_id"`
	ConversationID string    `json:"conversation_id"`
	UserID         string    `json:"user_id"`
	BodyText       string    `json:"body_text"`
	Internal       bool      `json:"internal"`
	UpdatedAt      time.Time `json:"updated_at"`
}

type ConversationDraftInput struct {
	OrgID          string
	ConversationID string
	UserID         string
	BodyText       string
	Internal       bool
}

type Ticket struct {
	ID                  string                   `json:"id"`
	OrgID               string                   `json:"org_id"`
	ConversationID      string                   `json:"conversation_id"`
	TicketKey           string                   `json:"ticket_key"`
	Status              string                   `json:"status"`
	WorkType            string                   `json:"work_type"`
	Priority            string                   `json:"priority"`
	Severity            string                   `json:"severity"`
	Category            string                   `json:"category,omitempty"`
	Intent              string                   `json:"intent,omitempty"`
	AssigneeUserID      string                   `json:"assignee_user_id,omitempty"`
	AssigneeName        string                   `json:"assignee_name,omitempty"`
	TeamID              string                   `json:"team_id,omitempty"`
	TeamName            string                   `json:"team_name,omitempty"`
	DueAt               *time.Time               `json:"due_at,omitempty"`
	FollowUpAt          *time.Time               `json:"follow_up_at,omitempty"`
	Source              string                   `json:"source"`
	AIConfidence        float64                  `json:"ai_confidence,omitempty"`
	AIReason            string                   `json:"ai_reason,omitempty"`
	CreatedBy           string                   `json:"created_by,omitempty"`
	WaitingSince        *time.Time               `json:"waiting_since,omitempty"`
	LastCustomerReplyAt *time.Time               `json:"last_customer_reply_at,omitempty"`
	FirstResponseAt     *time.Time               `json:"first_response_at,omitempty"`
	ResolvedAt          *time.Time               `json:"resolved_at,omitempty"`
	SnoozedUntil        *time.Time               `json:"snoozed_until,omitempty"`
	SLAPolicyID         string                   `json:"sla_policy_id,omitempty"`
	EscalationAt        *time.Time               `json:"escalation_at,omitempty"`
	Labels              []string                 `json:"labels"`
	SLAState            string                   `json:"sla_state"`
	Conversation        *ConversationSummary     `json:"conversation,omitempty"`
	LinkedResources     []TicketLinkedResource   `json:"linked_resources,omitempty"`
	Checklists          []TicketChecklist        `json:"checklists,omitempty"`
	SideConversations   []TicketSideConversation `json:"side_conversations,omitempty"`
	CreatedAt           time.Time                `json:"created_at"`
	UpdatedAt           time.Time                `json:"updated_at"`
}

// TicketSideConversation is a ticket-scoped internal coordination thread. It
// is deliberately distinct from a customer conversation and from Verevon Chat:
// it cannot select a channel, recipient, or delivery path.
type TicketSideConversation struct {
	ID              string                          `json:"id"`
	OrgID           string                          `json:"org_id"`
	TicketID        string                          `json:"ticket_id"`
	Subject         string                          `json:"subject"`
	Status          string                          `json:"status"`
	CreatedByUserID string                          `json:"created_by_user_id,omitempty"`
	Messages        []TicketSideConversationMessage `json:"messages"`
	CreatedAt       time.Time                       `json:"created_at"`
	UpdatedAt       time.Time                       `json:"updated_at"`
}

type TicketSideConversationMessage struct {
	ID                 string    `json:"id"`
	OrgID              string    `json:"org_id"`
	SideConversationID string    `json:"side_conversation_id"`
	BodyText           string    `json:"body_text"`
	CreatedByUserID    string    `json:"created_by_user_id,omitempty"`
	CreatedAt          time.Time `json:"created_at"`
}

// TicketActivity is a tenant-scoped, display-safe projection of a ticket audit
// record. Details are allow-listed from the audit payload rather than exposing
// arbitrary JSON written by an integration or automation.
type TicketActivity struct {
	ID           string    `json:"id"`
	Action       string    `json:"action"`
	ActorUserID  string    `json:"actor_user_id,omitempty"`
	ResourceKind string    `json:"resource_kind,omitempty"`
	CreatedAt    time.Time `json:"created_at"`
}

// ConversationActivity is the display-safe projection of an auditable Inbox
// event. It excludes all payload fields, message bodies, recipients, and
// provider identifiers; the UI gets only enough data to explain the work
// lifecycle without turning the audit log into another transcript.
type ConversationActivity struct {
	ID           string    `json:"id"`
	Action       string    `json:"action"`
	ActorUserID  string    `json:"actor_user_id,omitempty"`
	ResourceKind string    `json:"resource_kind,omitempty"`
	CreatedAt    time.Time `json:"created_at"`
}

type TicketLinkedResource struct {
	ID              string                  `json:"id"`
	OrgID           string                  `json:"org_id"`
	TicketID        string                  `json:"ticket_id"`
	ConversationID  string                  `json:"conversation_id"`
	LinkType        string                  `json:"link_type"`
	ResourceKind    string                  `json:"resource_kind"`
	ResourceID      string                  `json:"resource_id,omitempty"`
	ResourceURL     string                  `json:"resource_url,omitempty"`
	Label           string                  `json:"label,omitempty"`
	Metadata        map[string]any          `json:"metadata,omitempty"`
	LinkedTicket    *TicketDependencyTarget `json:"linked_ticket,omitempty"`
	CreatedByUserID string                  `json:"created_by_user_id,omitempty"`
	CreatedAt       time.Time               `json:"created_at"`
}

// TicketDependencyTarget is the bounded, canonical state exposed with a
// ticket-to-ticket link. It is dependency context, not a nested mutable
// resource and it does not imply lifecycle propagation between tickets.
type TicketDependencyTarget struct {
	ID        string `json:"id"`
	TicketKey string `json:"ticket_key"`
	Status    string `json:"status"`
	WorkType  string `json:"work_type"`
}

// Problem is a durable root-cause record. It is intentionally independent of
// any incident or ticket so a known error can outlive an individual event.
type Problem struct {
	ID              string     `json:"id"`
	OrgID           string     `json:"org_id"`
	ProblemKey      string     `json:"problem_key"`
	Title           string     `json:"title"`
	Status          string     `json:"status"`
	OwnerUserID     string     `json:"owner_user_id,omitempty"`
	OwnerName       string     `json:"owner_name,omitempty"`
	Summary         string     `json:"summary,omitempty"`
	RootCause       string     `json:"root_cause,omitempty"`
	CreatedByUserID string     `json:"created_by_user_id,omitempty"`
	ResolvedAt      *time.Time `json:"resolved_at,omitempty"`
	CreatedAt       time.Time  `json:"created_at"`
	UpdatedAt       time.Time  `json:"updated_at"`
}

// Incident is a declared operational event. A linked Problem is explanatory
// context only; resolving either record never changes the other by itself.
type Incident struct {
	ID               string               `json:"id"`
	OrgID            string               `json:"org_id"`
	IncidentKey      string               `json:"incident_key"`
	Title            string               `json:"title"`
	Status           string               `json:"status"`
	Severity         string               `json:"severity"`
	OwnerUserID      string               `json:"owner_user_id,omitempty"`
	OwnerName        string               `json:"owner_name,omitempty"`
	CustomerImpact   string               `json:"customer_impact,omitempty"`
	ProblemID        string               `json:"problem_id,omitempty"`
	DeclaredByUserID string               `json:"declared_by_user_id,omitempty"`
	DeclaredAt       time.Time            `json:"declared_at"`
	ResolvedAt       *time.Time           `json:"resolved_at,omitempty"`
	TicketLinks      []IncidentTicketLink `json:"ticket_links,omitempty"`
	CreatedAt        time.Time            `json:"created_at"`
	UpdatedAt        time.Time            `json:"updated_at"`
}

// IncidentTicketLink has explicit operational meaning; it is not a generic
// ticket dependency and it never propagates a lifecycle transition.
type IncidentTicketLink struct {
	ID              string    `json:"id"`
	OrgID           string    `json:"org_id"`
	IncidentID      string    `json:"incident_id"`
	TicketID        string    `json:"ticket_id"`
	TicketKey       string    `json:"ticket_key,omitempty"`
	TicketStatus    string    `json:"ticket_status,omitempty"`
	Relationship    string    `json:"relationship"`
	CreatedByUserID string    `json:"created_by_user_id,omitempty"`
	CreatedAt       time.Time `json:"created_at"`
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

// TicketTeam is the canonical Ticketing routing directory. It is deliberately
// distinct from a provider Inbox/group: integrations may expose source queues,
// but only this organization-scoped record may be selected as ticket ownership.
type TicketTeam struct {
	ID          string    `json:"id"`
	OrgID       string    `json:"org_id"`
	Name        string    `json:"name"`
	Description string    `json:"description,omitempty"`
	Active      bool      `json:"active"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
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
	ID                string `json:"id"`
	OrgID             string `json:"org_id"`
	ConversationID    string `json:"conversation_id"`
	Direction         string `json:"direction"`
	SenderType        string `json:"sender_type"`
	SenderName        string `json:"sender_name,omitempty"`
	SenderEmail       string `json:"sender_email,omitempty"`
	BodyText          string `json:"body_text"`
	BodyHTML          string `json:"body_html,omitempty"`
	Internal          bool   `json:"internal"`
	Provider          string `json:"provider,omitempty"`
	ProviderMessageID string `json:"provider_message_id,omitempty"`
	ProviderEventID   string `json:"provider_event_id,omitempty"`
	// Attachments expose only bounded presentation metadata. Provider and
	// storage references stay server-side until a separately authorized download
	// contract exists.
	Attachments []MessageAttachment `json:"attachments,omitempty"`
	OccurredAt  time.Time           `json:"occurred_at"`
	CreatedAt   time.Time           `json:"created_at"`
}

type MessageAttachment struct {
	ID        string `json:"id"`
	Filename  string `json:"filename"`
	MimeType  string `json:"mime_type,omitempty"`
	SizeBytes int64  `json:"size_bytes"`
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
	IDempotencyKey    string `json:"idempotency_key"`
	OrgID             string `json:"org_id"`
	ConnectionID      string `json:"connection_id,omitempty"`
	Provider          string `json:"provider"`
	ProviderEventID   string `json:"provider_event_id,omitempty"`
	ProviderMessageID string `json:"provider_message_id,omitempty"`
	ProviderThreadID  string `json:"provider_thread_id,omitempty"`
	// Email provenance is accepted only from the signed ingest bridge. It is
	// retained on the channel-thread ref for RFC-compliant Gmail replies, never
	// rendered as customer content or accepted from browser send requests.
	MessageIDHeader  string `json:"message_id_header,omitempty"`
	ReferencesHeader string `json:"references_header,omitempty"`
	InReplyToHeader  string `json:"in_reply_to_header,omitempty"`
	// Delivery-report metadata is accepted only from the signed email ingest
	// bridge. It can alter a ledger only with an exact opaque intent marker.
	AutoSubmitted         string             `json:"auto_submitted,omitempty"`
	ContentType           string             `json:"content_type,omitempty"`
	OutboundCorrelationID string             `json:"outbound_correlation_id,omitempty"`
	Direction             string             `json:"direction,omitempty"`
	Subject               string             `json:"subject"`
	From                  ParticipantInput   `json:"from"`
	To                    []ParticipantInput `json:"to,omitempty"`
	BodyText              string             `json:"body_text"`
	BodyHTML              string             `json:"body_html,omitempty"`
	Attachments           []AttachmentInput  `json:"attachments,omitempty"`
	OccurredAt            time.Time          `json:"occurred_at"`
}

type StoredEventResult struct {
	Detail  *ConversationDetail `json:"detail"`
	Message *Message            `json:"message"`
	Created bool                `json:"created"`
}

// FeedbackInput is a signed-in org member's one-line friction report,
// submitted from anywhere in the product via the persistent "Send feedback"
// control. Service.SubmitFeedback turns it into a normal inbound conversation
// (through the same path real provider webhooks use) tagged FeedbackTag, so
// the team reviews it in the org's own Inbox rather than a bespoke store --
// and mirrors it into the configured FEEDBACK_MIRROR_ORG_ID org so it stays
// visible to the team even when the submitter's org is an external pilot
// tenant (see Service.mirrorFeedback).
type FeedbackInput struct {
	OrgID       string
	ActorUserID string
	// FromName/FromEmail are display-only contact fields for the resulting
	// conversation's contact card, the same trust level as any other inbound
	// channel's sender fields (e.g. a webhook payload's From). They are never
	// used for authorization -- OrgID/ActorUserID come from the verified
	// gateway delegation, not from this input.
	FromName       string
	FromEmail      string
	BodyText       string
	IdempotencyKey string
	// PageURL is the route the submitter was on when they opened the feedback
	// widget (e.g. "/inbox?view=mine"), supplied by the client on a best-effort
	// basis. It is appended to the stored conversation body so a terse
	// one-line report still says where the friction happened; it is never
	// required and never validated as an authorization boundary.
	PageURL string
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
	// IdempotencyKey is a stable manual-send intent identifier supplied in the
	// signed request body and reused across retries. It becomes both the human
	// approval reference and the integration-corev2 receipt key.
	IdempotencyKey string
	// Provider and ProviderMessageID record the channel a human reply was
	// actually delivered through. They are set by the Service after a successful
	// integration-corev2 send so the stored outbound row reflects the real
	// delivery; they stay empty for internal notes and store-only conversations.
	Provider          string
	ProviderMessageID string
	OccurredAt        time.Time
}

// OutboundIntent contains only routing identifiers, hashes, and outcome state;
// message content is never duplicated into this ledger.
type OutboundIntent struct {
	ID                         string
	OrgID                      string
	IdempotencyKey             string
	ConversationID             string
	AIActionID                 string
	RequestFingerprint         string
	Status                     string
	Provider                   string
	ConnectionID               string
	ProviderThreadID           string
	AuthorizationKind          string
	ActorUserID                string
	ApprovalID                 string
	ActionID                   string
	Operation                  string
	PayloadSHA256              string
	ProviderMessageID          string
	ProviderDeliveryStatus     string
	ProviderDeliveryOccurredAt *time.Time
	ProviderDeliveryErrorCode  string
	MessageID                  string
	ErrorCode                  string
	CreatedAt                  time.Time
	UpdatedAt                  time.Time
}

// OutboundIntentListFilter selects content-free outbound receipts for one
// organization. It intentionally cannot search message text, recipients, or
// provider-thread identifiers.
type OutboundIntentListFilter struct {
	OrgID          string
	Status         string
	Provider       string
	DeliveryStatus string
	Limit          int
}

type OutboundIntentClaimInput struct {
	IntentID           string
	OrgID              string
	IdempotencyKey     string
	ConversationID     string
	AIActionID         string
	RequestFingerprint string
	Provider           string
	ConnectionID       string
	ProviderThreadID   string
	AuthorizationKind  string
	ActorUserID        string
	ApprovalID         string
	ActionID           string
	Operation          string
	PayloadSHA256      string
}

type OutboundIntentClaim struct {
	Intent  OutboundIntent
	Claimed bool
}

type OutboundIntentFinalizeInput struct {
	OrgID              string
	IdempotencyKey     string
	RequestFingerprint string
	AIActionID         string
	Message            AddMessageInput
	ProviderMessageID  string
}

type OutboundIntentOutcomeInput struct {
	OrgID          string
	IdempotencyKey string
	AIActionID     string
	Status         string
	ErrorCode      string
}

// ProviderDeliveryReceiptInput is a normalized provider callback. It contains
// only the exact provider message identifier and bounded status metadata, so a
// callback cannot invent a delivery state for another tenant or transcript.
type ProviderDeliveryReceiptInput struct {
	OrgID             string
	Provider          string
	ProviderMessageID string
	Status            string
	OccurredAt        time.Time
	ErrorCode         string
}

// EmailDeliveryFailureInput is accepted only after the signed email bridge
// proves a machine-readable delivery-status report and extracts the opaque
// outbound intent marker. It deliberately has no provider diagnostic text.
type EmailDeliveryFailureInput struct {
	OrgID            string
	Provider         string
	OutboundIntentID string
	OccurredAt       time.Time
}

type ListFilter struct {
	OrgID         string
	InboxID       string
	ConnectionID  string
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
	WorkType string
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
	// EditedFields carries a reviewer's overrides for an approval, keyed by the
	// same field names promote() reads from payload.suggested_fields (category,
	// priority, severity, intent, team_id, team_name). Only present for
	// decision == "approved"; Service.ReviewAIAction whitelist-filters it before
	// it reaches the repository, which merges it into suggested_fields inside
	// the same transaction as the status update. A reject must never carry a
	// non-empty value here.
	EditedFields map[string]string
	OccurredAt   time.Time
}

// AIAction is a model-proposed action awaiting (or having received) a human
// review decision — the read shape backing the HITL review queue. It mirrors a
// row of conversation_ai_actions.
type AIAction struct {
	ID             string `json:"id"`
	OrgID          string `json:"org_id"`
	ConversationID string `json:"conversation_id"`
	// ProposalGroupID is an immutable, opaque correlation key for separately
	// reviewed actions prepared by one resolution plan. It has no authority to
	// approve, reject, or execute any member.
	ProposalGroupID string         `json:"proposal_group_id,omitempty"`
	Kind            string         `json:"kind"`
	Status          string         `json:"status"`
	Payload         map[string]any `json:"payload"`
	CreatedBy       string         `json:"created_by"`
	ReviewedBy      string         `json:"reviewed_by,omitempty"`
	ReviewedAt      *time.Time     `json:"reviewed_at,omitempty"`
	CreatedAt       time.Time      `json:"created_at"`
	UpdatedAt       time.Time      `json:"updated_at"`
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
	OrgID           string
	ConversationID  string
	ProposalGroupID string
	Kind            string
	Payload         map[string]any
	CreatedBy       string
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
	ReplyToMessageID string `json:"-"`
	ReferencesHeader string `json:"-"`
}

type CreateTicketInput struct {
	OrgID               string
	ConversationID      string
	Status              string
	WorkType            string
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
	// Operation fields are set by the owning Service, never trusted from HTTP.
	// The durable repository binds them to one normalized ticket request.
	ActionID       string
	IdempotencyKey string
	OperationID    string
	RequestSHA256  string
}

// TicketOperationReceipt is the durable result of the owner-plane
// `tickets.create` operation. It deliberately distinguishes a replay from a
// new write and exposes stable owner identifiers rather than gateway-derived
// run/audit strings.
type TicketOperationReceipt struct {
	OperationID  string  `json:"operation_id"`
	AuditEventID string  `json:"audit_event_id"`
	Status       string  `json:"status"`
	Ticket       *Ticket `json:"ticket"`
	Replayed     bool    `json:"replayed"`
}

type UpdateTicketInput struct {
	OrgID               string
	TicketID            string
	Status              *string
	WorkType            *string
	Priority            *string
	Severity            *string
	Category            *string
	Intent              *string
	AssigneeUserID      *string
	AssigneeName        *string
	TeamID              *string
	TeamName            *string
	DueAt               *time.Time
	FollowUpAt          *time.Time
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

type CreateIncidentInput struct {
	OrgID            string
	Title            string
	Status           string
	Severity         string
	OwnerUserID      string
	OwnerName        string
	CustomerImpact   string
	ProblemID        string
	DeclaredByUserID string
}

type ApprovedIncidentCreateInput struct {
	OrgID          string
	ConversationID string
	AIActionID     string
	TicketID       string
	Title          string
	Severity       string
	CustomerImpact string
	ActorUserID    string
}

type ApprovedProblemCreateInput struct {
	OrgID          string
	ConversationID string
	AIActionID     string
	Title          string
	Summary        string
	RootCause      string
	ActorUserID    string
}

type UpdateIncidentInput struct {
	OrgID          string
	IncidentID     string
	Title          *string
	Status         *string
	Severity       *string
	OwnerUserID    *string
	OwnerName      *string
	CustomerImpact *string
	ProblemID      *string
	ActorUserID    string
}

type LinkIncidentTicketInput struct {
	OrgID           string
	IncidentID      string
	TicketID        string
	Relationship    string
	CreatedByUserID string
}

type CreateProblemInput struct {
	OrgID           string
	Title           string
	Status          string
	OwnerUserID     string
	OwnerName       string
	Summary         string
	RootCause       string
	CreatedByUserID string
}

type UpdateProblemInput struct {
	OrgID       string
	ProblemID   string
	Title       *string
	Status      *string
	OwnerUserID *string
	OwnerName   *string
	Summary     *string
	RootCause   *string
	ActorUserID string
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

type CreateTicketTeamInput struct {
	OrgID       string
	Name        string
	Description string
	Active      bool
	ActorUserID string
}

type UpdateTicketTeamInput struct {
	OrgID       string
	ID          string
	Name        *string
	Description *string
	Active      *bool
	ActorUserID string
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
	OrgID                  string
	TicketID               string
	MacroID                string
	ActorUserID            string
	ExpectedMacroUpdatedAt string
	Actions                map[string]any
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

type CreateTicketSideConversationInput struct {
	OrgID       string
	TicketID    string
	Subject     string
	BodyText    string
	ActorUserID string
}

type AddTicketSideConversationMessageInput struct {
	OrgID              string
	TicketID           string
	SideConversationID string
	BodyText           string
	ActorUserID        string
}

type UpdateTicketSideConversationInput struct {
	OrgID              string
	TicketID           string
	SideConversationID string
	Status             string
	ActorUserID        string
}

// TicketChatHandoffInput records an operator's request to open the separately
// owned Chat surface. It does not represent a Chat action, response, or
// customer delivery receipt.
type TicketChatHandoffInput struct {
	OrgID       string
	TicketID    string
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

// EventID lets transport publishers apply broker-side de-duplication without
// importing the conversation package or depending on a concrete event type.
func (event LifecycleEvent) EventID() string { return event.ID }

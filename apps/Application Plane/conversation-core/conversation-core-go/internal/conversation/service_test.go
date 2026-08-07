package conversation

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/attestation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/integration"
)

type fakeRepository struct {
	mu                      sync.Mutex
	inboxes                 []Inbox
	conversations           []ConversationSummary
	details                 map[string]*ConversationDetail
	stored                  map[string]*StoredEventResult
	lastMessage             AddMessageInput
	addMessageCalls         int
	addMessageErr           error
	threadRefs              map[string]*ChannelThreadRef
	threadRefErr            error
	statusUpdate            StatusUpdate
	tickets                 map[string]*Ticket
	macros                  map[string]*TicketMacro
	checklists              map[string]*TicketChecklist
	rules                   []TicketAutomationRule
	classifications         []TicketClassificationInput
	aiActions               []AIAction
	lastReview              AIActionReview
	reviewCalls             int
	reviewErr               error
	outboundIntents         map[string]*OutboundIntent
	outboundMessages        map[string]*Message
	reconcileResult         []OutboundIntent
	reconcileErr            error
	reconcileCalls          []time.Duration
	hardPurgeCalls          []string
	hardPurgeErr            error
	storeInboundCalls       int
	supportRecurrenceCorpus []SupportRecurrenceCorpusEntry
}

func newFakeRepository() *fakeRepository {
	return &fakeRepository{
		details:          make(map[string]*ConversationDetail),
		stored:           make(map[string]*StoredEventResult),
		threadRefs:       make(map[string]*ChannelThreadRef),
		tickets:          make(map[string]*Ticket),
		macros:           make(map[string]*TicketMacro),
		checklists:       make(map[string]*TicketChecklist),
		outboundIntents:  make(map[string]*OutboundIntent),
		outboundMessages: make(map[string]*Message),
	}
}

// fakeSender records outbound send attempts so tests can assert whether a reply
// was actually delivered and with what request shape.
type fakeSender struct {
	mu      sync.Mutex
	calls   int
	lastReq integration.SendRequest
	result  *integration.SendResult
	err     error
}

func (f *fakeSender) Send(_ context.Context, req integration.SendRequest) (*integration.SendResult, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls++
	f.lastReq = req
	if f.err != nil {
		return nil, f.err
	}
	if f.result != nil {
		return f.result, nil
	}
	return &integration.SendResult{}, nil
}

func (f *fakeSender) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.calls
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
	f.storeInboundCalls++
	if existing, ok := f.stored[event.IDempotencyKey]; ok {
		next := *existing
		next.Created = false
		return &next, nil
	}
	detail := &ConversationDetail{
		ConversationSummary: ConversationSummary{ID: "conv_1", OrgID: event.OrgID, Title: event.Subject, Provider: event.Provider},
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
	// Mirrors PGRepository: a conversation just created by StoreInboundEvent is
	// immediately visible to GetConversation/AddTag/etc, not only through the
	// idempotency-keyed StoredEventResult cache above.
	f.details[detail.ID] = detail
	return result, nil
}

func (f *fakeRepository) AddMessage(_ context.Context, input AddMessageInput) (*Message, error) {
	f.addMessageCalls++
	f.lastMessage = input
	if f.addMessageErr != nil {
		return nil, f.addMessageErr
	}
	return &Message{
		ID:                "msg_reply",
		OrgID:             input.OrgID,
		ConversationID:    input.ConversationID,
		Direction:         input.Direction,
		BodyText:          input.BodyText,
		Internal:          input.Internal,
		Provider:          input.Provider,
		ProviderMessageID: input.ProviderMessageID,
		OccurredAt:        input.OccurredAt,
		CreatedAt:         input.OccurredAt,
	}, nil
}

func (f *fakeRepository) ClaimOutboundIntent(_ context.Context, input OutboundIntentClaimInput) (*OutboundIntentClaim, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	key := input.OrgID + ":" + input.IdempotencyKey
	if existing := f.outboundIntents[key]; existing != nil {
		if !outboundIntentBindingMatches(*existing, input) {
			return nil, ErrConflict
		}
		if existing.Status == OutboundIntentRetryable {
			existing.Status = OutboundIntentSending
			return &OutboundIntentClaim{Intent: *existing, Claimed: true}, nil
		}
		return &OutboundIntentClaim{Intent: *existing, Claimed: false}, nil
	}
	intent := &OutboundIntent{
		ID: input.IntentID, OrgID: input.OrgID, IdempotencyKey: input.IdempotencyKey,
		ConversationID: input.ConversationID, AIActionID: input.AIActionID,
		RequestFingerprint: input.RequestFingerprint, Status: OutboundIntentSending,
		Provider: input.Provider, ConnectionID: input.ConnectionID, ProviderThreadID: input.ProviderThreadID,
		AuthorizationKind: input.AuthorizationKind, ActorUserID: input.ActorUserID,
		ApprovalID: input.ApprovalID, ActionID: input.ActionID, Operation: input.Operation, PayloadSHA256: input.PayloadSHA256,
	}
	f.outboundIntents[key] = intent
	return &OutboundIntentClaim{Intent: *intent, Claimed: true}, nil
}

func (f *fakeRepository) FinalizeOutboundIntent(_ context.Context, input OutboundIntentFinalizeInput) (*Message, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.addMessageCalls++
	if f.addMessageErr != nil {
		f.lastMessage = input.Message
		return nil, f.addMessageErr
	}
	key := input.OrgID + ":" + input.IdempotencyKey
	intent := f.outboundIntents[key]
	if intent == nil || intent.Status != OutboundIntentSending || intent.RequestFingerprint != input.RequestFingerprint {
		return nil, ErrConflict
	}
	f.lastMessage = input.Message
	f.lastMessage.Provider = intent.Provider
	f.lastMessage.ProviderMessageID = input.ProviderMessageID
	message := &Message{
		ID: "msg_reply", OrgID: input.Message.OrgID, ConversationID: input.Message.ConversationID,
		Direction: input.Message.Direction, BodyText: input.Message.BodyText,
		Provider: intent.Provider, ProviderMessageID: input.ProviderMessageID,
		OccurredAt: input.Message.OccurredAt, CreatedAt: input.Message.OccurredAt,
	}
	intent.Status = OutboundIntentSubmitted
	intent.ProviderMessageID = input.ProviderMessageID
	intent.MessageID = message.ID
	f.outboundMessages[input.OrgID+":"+message.ID] = message
	return message, nil
}

func (f *fakeRepository) MarkOutboundIntentOutcome(_ context.Context, input OutboundIntentOutcomeInput) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	intent := f.outboundIntents[input.OrgID+":"+input.IdempotencyKey]
	if intent == nil {
		return ErrNotFound
	}
	if intent.Status != OutboundIntentSending && intent.Status != input.Status {
		return ErrConflict
	}
	intent.Status = input.Status
	intent.ErrorCode = input.ErrorCode
	return nil
}

func (f *fakeRepository) ReconcileStaleOutboundIntents(_ context.Context, staleAfter time.Duration) ([]OutboundIntent, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.reconcileCalls = append(f.reconcileCalls, staleAfter)
	if f.reconcileErr != nil {
		return nil, f.reconcileErr
	}
	return f.reconcileResult, nil
}

func (f *fakeRepository) GetMessage(_ context.Context, orgID, messageID string) (*Message, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	message := f.outboundMessages[orgID+":"+messageID]
	if message == nil {
		return nil, ErrNotFound
	}
	copy := *message
	return &copy, nil
}

func (f *fakeRepository) GetChannelThreadRefByConversation(_ context.Context, _ string, conversationID string) (*ChannelThreadRef, error) {
	if f.threadRefErr != nil {
		return nil, f.threadRefErr
	}
	ref, ok := f.threadRefs[conversationID]
	if !ok {
		return nil, ErrNotFound
	}
	return ref, nil
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

func (f *fakeRepository) ReviewAIAction(_ context.Context, input AIActionReview) error {
	f.reviewCalls++
	f.lastReview = input
	return f.reviewErr
}

func (f *fakeRepository) CreateAIAction(_ context.Context, input CreateAIActionInput) (*AIAction, error) {
	action := AIAction{
		ID:             "act-test",
		OrgID:          input.OrgID,
		ConversationID: input.ConversationID,
		Kind:           input.Kind,
		Status:         "suggested",
		Payload:        input.Payload,
		CreatedBy:      input.CreatedBy,
	}
	f.aiActions = append(f.aiActions, action)
	return &action, nil
}

func (f *fakeRepository) ListAIActions(_ context.Context, filter AIActionListFilter) ([]AIAction, error) {
	out := []AIAction{}
	for _, action := range f.aiActions {
		if action.OrgID != filter.OrgID {
			continue
		}
		if filter.Status != "" && action.Status != filter.Status {
			continue
		}
		if filter.ConversationID != "" && action.ConversationID != filter.ConversationID {
			continue
		}
		out = append(out, action)
	}
	return out, nil
}

func (f *fakeRepository) ListTickets(_ context.Context, _ TicketListFilter) ([]Ticket, error) {
	tickets := []Ticket{}
	for _, ticket := range f.tickets {
		tickets = append(tickets, *ticket)
	}
	return tickets, nil
}

func (f *fakeRepository) GetTicket(_ context.Context, _ string, ticketID string) (*Ticket, error) {
	ticket, ok := f.tickets[ticketID]
	if !ok {
		return nil, ErrNotFound
	}
	return ticket, nil
}

func (f *fakeRepository) GetTicketByConversation(_ context.Context, _ string, conversationID string) (*Ticket, error) {
	for _, ticket := range f.tickets {
		if ticket.ConversationID == conversationID {
			return ticket, nil
		}
	}
	return nil, ErrNotFound
}

func (f *fakeRepository) CreateTicket(_ context.Context, input CreateTicketInput) (*Ticket, error) {
	for _, ticket := range f.tickets {
		if ticket.ConversationID == input.ConversationID {
			return nil, ErrConflict
		}
	}
	now := time.Date(2026, time.June, 3, 12, 20, 0, 0, time.UTC)
	ticket := &Ticket{
		ID:             "ticket_1",
		OrgID:          input.OrgID,
		ConversationID: input.ConversationID,
		TicketKey:      "TCK-FAKE",
		Status:         input.Status,
		Priority:       input.Priority,
		Severity:       input.Severity,
		Category:       input.Category,
		Intent:         input.Intent,
		Source:         input.Source,
		AIConfidence:   input.AIConfidence,
		AIReason:       input.AIReason,
		CreatedBy:      input.CreatedBy,
		SnoozedUntil:   input.SnoozedUntil,
		SLAPolicyID:    input.SLAPolicyID,
		Labels:         input.Labels,
		SLAState:       "ok",
		CreatedAt:      now,
		UpdatedAt:      now,
	}
	f.tickets[ticket.ID] = ticket
	return ticket, nil
}

func (f *fakeRepository) UpdateTicket(_ context.Context, input UpdateTicketInput) (*Ticket, error) {
	ticket, ok := f.tickets[input.TicketID]
	if !ok {
		return nil, ErrNotFound
	}
	if input.Status != nil {
		ticket.Status = *input.Status
	}
	if input.Priority != nil {
		ticket.Priority = *input.Priority
	}
	if input.TeamID != nil {
		ticket.TeamID = *input.TeamID
	}
	if input.Labels != nil {
		ticket.Labels = *input.Labels
	}
	if input.SnoozedUntil != nil {
		ticket.SnoozedUntil = input.SnoozedUntil
	}
	return ticket, nil
}

func (f *fakeRepository) LinkTicketResource(_ context.Context, input LinkTicketResourceInput) (*TicketLinkedResource, error) {
	ticket, ok := f.tickets[input.TicketID]
	if !ok {
		return nil, ErrNotFound
	}
	return &TicketLinkedResource{
		ID:             "link_1",
		OrgID:          input.OrgID,
		TicketID:       input.TicketID,
		ConversationID: ticket.ConversationID,
		LinkType:       input.LinkType,
		ResourceKind:   input.ResourceKind,
		ResourceID:     input.ResourceID,
		Metadata:       input.Metadata,
		CreatedAt:      time.Date(2026, time.June, 3, 12, 30, 0, 0, time.UTC),
	}, nil
}

func (f *fakeRepository) ListTicketViews(_ context.Context, _ string) ([]TicketView, error) {
	return []TicketView{}, nil
}

func (f *fakeRepository) CreateTicketView(_ context.Context, input CreateTicketViewInput) (*TicketView, error) {
	return &TicketView{ID: "view_1", OrgID: input.OrgID, Name: input.Name, Filter: input.Filter, Sort: input.Sort}, nil
}

func (f *fakeRepository) UpdateTicketView(_ context.Context, input UpdateTicketViewInput) (*TicketView, error) {
	name := ""
	if input.Name != nil {
		name = *input.Name
	}
	return &TicketView{ID: input.ID, OrgID: input.OrgID, Name: name, Filter: map[string]any{}, Sort: map[string]any{}}, nil
}

func (f *fakeRepository) ListTicketMacros(_ context.Context, _ string) ([]TicketMacro, error) {
	items := []TicketMacro{}
	for _, item := range f.macros {
		items = append(items, *item)
	}
	return items, nil
}

func (f *fakeRepository) GetTicketMacro(_ context.Context, _ string, macroID string) (*TicketMacro, error) {
	item, ok := f.macros[macroID]
	if !ok {
		return nil, ErrNotFound
	}
	return item, nil
}

func (f *fakeRepository) CreateTicketMacro(_ context.Context, input CreateTicketMacroInput) (*TicketMacro, error) {
	item := &TicketMacro{ID: "macro_1", OrgID: input.OrgID, Name: input.Name, Active: input.Active, Actions: input.Actions, Conditions: input.Conditions}
	f.macros[item.ID] = item
	return item, nil
}

func (f *fakeRepository) UpdateTicketMacro(_ context.Context, input UpdateTicketMacroInput) (*TicketMacro, error) {
	item, ok := f.macros[input.ID]
	if !ok {
		return nil, ErrNotFound
	}
	if input.Actions != nil {
		item.Actions = *input.Actions
	}
	return item, nil
}

func (f *fakeRepository) RecordTicketMacroRun(_ context.Context, _ TicketMacroRunInput) error {
	return nil
}

func (f *fakeRepository) ListTicketAutomationRules(_ context.Context, _ string) ([]TicketAutomationRule, error) {
	return f.rules, nil
}

func (f *fakeRepository) CreateTicketAutomationRule(_ context.Context, input CreateTicketAutomationRuleInput) (*TicketAutomationRule, error) {
	return &TicketAutomationRule{ID: "rule_1", OrgID: input.OrgID, Name: input.Name, EventName: input.EventName, Active: input.Active, Conditions: input.Conditions, Actions: input.Actions}, nil
}

func (f *fakeRepository) UpdateTicketAutomationRule(_ context.Context, input UpdateTicketAutomationRuleInput) (*TicketAutomationRule, error) {
	return &TicketAutomationRule{ID: input.ID, OrgID: input.OrgID, Conditions: map[string]any{}, Actions: map[string]any{}}, nil
}

func (f *fakeRepository) ListSLAPolicies(_ context.Context, _ string) ([]SLAPolicy, error) {
	return []SLAPolicy{}, nil
}

func (f *fakeRepository) CreateSLAPolicy(_ context.Context, input CreateSLAPolicyInput) (*SLAPolicy, error) {
	return &SLAPolicy{ID: "sla_1", OrgID: input.OrgID, Name: input.Name, Active: input.Active, Conditions: input.Conditions}, nil
}

func (f *fakeRepository) UpdateSLAPolicy(_ context.Context, input UpdateSLAPolicyInput) (*SLAPolicy, error) {
	return &SLAPolicy{ID: input.ID, OrgID: input.OrgID, Conditions: map[string]any{}}, nil
}

func (f *fakeRepository) CreateTicketChecklist(_ context.Context, input CreateTicketChecklistInput) (*TicketChecklist, error) {
	checklist := &TicketChecklist{
		ID:              "checklist_1",
		OrgID:           input.OrgID,
		TicketID:        input.TicketID,
		Name:            input.Name,
		CreatedByUserID: input.CreatedByUserID,
		CreatedAt:       time.Date(2026, time.June, 3, 12, 40, 0, 0, time.UTC),
		UpdatedAt:       time.Date(2026, time.June, 3, 12, 40, 0, 0, time.UTC),
	}
	for idx, label := range input.Items {
		checklist.Items = append(checklist.Items, TicketChecklistItem{
			ID:          "item_" + label,
			OrgID:       input.OrgID,
			ChecklistID: checklist.ID,
			Label:       label,
			Position:    idx,
		})
	}
	f.checklists[checklist.ID] = checklist
	return checklist, nil
}

func (f *fakeRepository) UpdateTicketChecklistItem(_ context.Context, input UpdateTicketChecklistItemInput) (*TicketChecklist, error) {
	checklist, ok := f.checklists[input.ChecklistID]
	if !ok {
		return nil, ErrNotFound
	}
	for idx := range checklist.Items {
		if checklist.Items[idx].ID == input.ItemID {
			checklist.Items[idx].Completed = input.Completed
		}
	}
	return checklist, nil
}

func (f *fakeRepository) HardPurgeByOrg(_ context.Context, orgID string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.hardPurgeErr != nil {
		return f.hardPurgeErr
	}
	f.hardPurgeCalls = append(f.hardPurgeCalls, orgID)
	return nil
}

func (f *fakeRepository) DistinctOrgIDsWithActiveTickets(_ context.Context) ([]string, error) {
	return nil, nil
}

func (f *fakeRepository) ActiveTicketsForSupportRecurrenceCorpus(_ context.Context, _ string) ([]Ticket, error) {
	return nil, nil
}

func (f *fakeRepository) UpsertSupportRecurrenceCorpusEntry(_ context.Context, _, _ string, _ []float32, _ string, _ time.Time) error {
	return nil
}

func (f *fakeRepository) EvictStaleSupportRecurrenceCorpusEntries(_ context.Context, _ string, _ time.Time) error {
	return nil
}

func (f *fakeRepository) PurgeSupportRecurrenceCorpusByOrg(_ context.Context, _ string) error {
	return nil
}

func (f *fakeRepository) ListSupportRecurrenceCorpus(_ context.Context, _ string) ([]SupportRecurrenceCorpusEntry, error) {
	return f.supportRecurrenceCorpus, nil
}

func (f *fakeRepository) RecordTicketClassification(_ context.Context, input TicketClassificationInput, payload map[string]any) (*TicketClassification, error) {
	f.classifications = append(f.classifications, input)
	return &TicketClassification{
		ID:             "aiact_1",
		OrgID:          input.OrgID,
		ConversationID: input.ConversationID,
		Outcome:        input.Outcome,
		Confidence:     input.Confidence,
		Reason:         input.Reason,
		Payload:        payload,
		CreatedAt:      time.Date(2026, time.June, 3, 12, 15, 0, 0, time.UTC),
	}, nil
}

type fakePublisher struct {
	subjects []string
	events   []LifecycleEvent
}

func (f *fakePublisher) Publish(_ context.Context, subject string, payload any) error {
	f.subjects = append(f.subjects, subject)
	if event, ok := payload.(LifecycleEvent); ok {
		f.events = append(f.events, event)
	}
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

func TestSanitizeStorableText(t *testing.T) {
	cases := []struct {
		name string
		in   string
		want string
	}{
		{"plain", "hello world", "hello world"},
		{"trims", "  hi  ", "hi"},
		{"strips nul", "hello\x00world", "helloworld"},
		{"keeps tab/newline", "line1\n\tline2", "line1\n\tline2"},
		{"strips c0 controls", "a\x01b\x1fc", "abc"},
		{"strips c1 controls", "abc", "abc"},
		{"strips replacement char", "a�b", "ab"},
		{"trims after strip", "\x00  spaced  \x00", "spaced"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := sanitizeStorableText(tc.in); got != tc.want {
				t.Fatalf("sanitizeStorableText(%q) = %q, want %q", tc.in, got, tc.want)
			}
		})
	}
}

// A message body carrying a NUL byte (routine in Outlook/Graph HTML) must not
// 500 the ingest and stall the poller — Postgres rejects 0x00 in TEXT. The
// stored message is sanitized instead.
func TestIngestEventStripsControlBytesFromBody(t *testing.T) {
	service := NewService(newFakeRepository(), nil)
	result, err := service.IngestEvent(context.Background(), InboundEvent{
		OrgID:             "org_1",
		Provider:          "microsoft",
		ProviderEventID:   "evt_nul",
		ProviderMessageID: "m_nul",
		Subject:           "Re: report\x00",
		From:              ParticipantInput{Name: "Ada\x00", Email: "ada@example.com"},
		BodyText:          "before\x00after",
		BodyHTML:          "<p>hi\x00</p>",
	})
	if err != nil {
		t.Fatalf("IngestEvent() error = %v, want nil", err)
	}
	if strings.ContainsRune(result.Message.BodyText, 0) || strings.ContainsRune(result.Message.BodyHTML, 0) {
		t.Fatalf("stored message still contains NUL: text=%q html=%q", result.Message.BodyText, result.Message.BodyHTML)
	}
	if result.Message.BodyText != "beforeafter" {
		t.Fatalf("body_text = %q, want beforeafter", result.Message.BodyText)
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

func TestIngestEventRejectsOutboundDirection(t *testing.T) {
	service := NewService(newFakeRepository(), nil)
	_, err := service.IngestEvent(t.Context(), InboundEvent{
		OrgID:          "org_1",
		IDempotencyKey: "email-outbound-attempt-0001",
		Direction:      DirectionOutbound,
		Subject:        "Forged sent state",
		From:           ParticipantInput{Email: "attacker@example.com"},
		BodyText:       "This was never sent by a provider.",
	})
	if !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("IngestEvent() error = %v, want ErrInvalidInput", err)
	}
}

func TestIngestEventAcceptsSyncedTeamsOutboundDirection(t *testing.T) {
	publisher := &fakePublisher{}
	service := NewService(newFakeRepository(), publisher)
	result, err := service.IngestEvent(t.Context(), InboundEvent{
		OrgID:             "org_1",
		Provider:          "teams",
		ProviderEventID:   "teams-message-1",
		ProviderMessageID: "teams-message-1",
		ProviderThreadID:  "teams-chat-1",
		Direction:         DirectionOutbound,
		Subject:           "Robert Røsten",
		From:              ParticipantInput{Name: "Ima Fernandes Da Costa", Email: "ima@aquatiq.com"},
		To:                []ParticipantInput{{Name: "Robert Røsten", Email: "robert@example.com"}},
		BodyText:          "My synced Teams reply",
	})
	if err != nil {
		t.Fatalf("IngestEvent() error = %v, want trusted Teams outbound history", err)
	}
	if result.Message.Direction != DirectionOutbound {
		t.Fatalf("stored direction = %q, want outbound", result.Message.Direction)
	}
	if len(publisher.subjects) != 1 || publisher.subjects[0] != SubjectMessageSent {
		t.Fatalf("subjects = %#v, want message.sent for synced self-authored history", publisher.subjects)
	}
}

func TestSubmitFeedbackCreatesTaggedConversation(t *testing.T) {
	repository := newFakeRepository()
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	detail, err := service.SubmitFeedback(t.Context(), FeedbackInput{
		OrgID:          "org_1",
		ActorUserID:    "user_1",
		FromName:       "Ada",
		FromEmail:      "ada@example.com",
		BodyText:       "The knowledge tab spinner never resolves.",
		IdempotencyKey: "feedback-0001",
	})
	if err != nil {
		t.Fatalf("SubmitFeedback() error = %v", err)
	}
	if detail == nil {
		t.Fatal("detail = nil, want a created conversation")
	}
	if len(detail.Tags) != 1 || detail.Tags[0] != FeedbackTag {
		t.Fatalf("tags = %#v, want exactly [%q]", detail.Tags, FeedbackTag)
	}
	stored, ok := repository.stored["feedback-0001"]
	if !ok {
		t.Fatal("event was not stored under the caller's idempotency key")
	}
	if stored.Message.BodyText != "The knowledge tab spinner never resolves." {
		t.Fatalf("stored body_text = %q, want the feedback note verbatim", stored.Message.BodyText)
	}
	if stored.Detail.Provider != FeedbackProvider {
		t.Fatalf("provider = %q, want %q", stored.Detail.Provider, FeedbackProvider)
	}
	// message.received (from IngestEvent) + tag.added (from AddTag). No
	// FEEDBACK_MIRROR_ORG_ID is configured here, so no mirror events follow.
	if len(publisher.subjects) != 2 || publisher.subjects[0] != SubjectMessageReceived || publisher.subjects[1] != SubjectTagAdded {
		t.Fatalf("subjects = %#v, want [message.received, tag.added]", publisher.subjects)
	}
}

func TestSubmitFeedbackAppendsPageURLToStoredBody(t *testing.T) {
	repository := newFakeRepository()
	service := NewService(repository, nil)

	_, err := service.SubmitFeedback(t.Context(), FeedbackInput{
		OrgID:          "org_1",
		BodyText:       "The search box loses focus after every keystroke.",
		IdempotencyKey: "feedback-page-0001",
		PageURL:        "/inbox?view=mine",
	})
	if err != nil {
		t.Fatalf("SubmitFeedback() error = %v", err)
	}
	stored, ok := repository.stored["feedback-page-0001"]
	if !ok {
		t.Fatal("event was not stored under the caller's idempotency key")
	}
	if !strings.Contains(stored.Message.BodyText, "/inbox?view=mine") {
		t.Fatalf("stored body_text = %q, want the reporting page URL included", stored.Message.BodyText)
	}
}

func TestSubmitFeedbackDefaultsSenderIdentityWhenClientOmitsIt(t *testing.T) {
	service := NewService(newFakeRepository(), nil)

	_, err := service.SubmitFeedback(t.Context(), FeedbackInput{
		OrgID:          "org_1",
		BodyText:       "No display name or email supplied.",
		IdempotencyKey: "feedback-anon-0001",
	})
	if err != nil {
		t.Fatalf("SubmitFeedback() error = %v, want a default sender identity to satisfy validateInboundEvent", err)
	}
}

func TestSubmitFeedbackRejectsEmptyBody(t *testing.T) {
	service := NewService(newFakeRepository(), nil)

	_, err := service.SubmitFeedback(t.Context(), FeedbackInput{
		OrgID:          "org_1",
		BodyText:       "   ",
		IdempotencyKey: "feedback-0002",
	})
	if !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("error = %v, want ErrInvalidInput", err)
	}
}

// Without a caller-supplied idempotency key, feedback has no provider
// event/message/thread id for IngestEvent's fallback derivation to key on --
// every submission from the same org would collapse onto the same key and
// silently drop every submission after the first. SubmitFeedback must
// reject this before it ever reaches IngestEvent.
func TestSubmitFeedbackRejectsMissingIdempotencyKey(t *testing.T) {
	service := NewService(newFakeRepository(), nil)

	_, err := service.SubmitFeedback(t.Context(), FeedbackInput{
		OrgID:    "org_1",
		BodyText: "Missing an idempotency key.",
	})
	if !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("error = %v, want ErrInvalidInput", err)
	}
}

func TestSubmitFeedbackReplayDoesNotDoubleTag(t *testing.T) {
	repository := newFakeRepository()
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)
	input := FeedbackInput{
		OrgID:          "org_1",
		ActorUserID:    "user_1",
		BodyText:       "Retried after a flaky network response.",
		IdempotencyKey: "feedback-retry-0001",
	}

	first, err := service.SubmitFeedback(t.Context(), input)
	if err != nil {
		t.Fatalf("first SubmitFeedback() error = %v", err)
	}
	second, err := service.SubmitFeedback(t.Context(), input)
	if err != nil {
		t.Fatalf("second SubmitFeedback() error = %v", err)
	}
	if second.ID != first.ID {
		t.Fatalf("replay conversation id = %q, want the original %q", second.ID, first.ID)
	}
	if len(second.Tags) != 1 {
		t.Fatalf("tags after replay = %#v, want exactly one feedback tag, not a duplicate", second.Tags)
	}
	if len(publisher.subjects) != 2 {
		t.Fatalf("subjects after replay = %#v, want unchanged from the first submission", publisher.subjects)
	}
}

// TestSubmitFeedbackMirrorsToConfiguredOrg covers the Phase 4 open-pilot fix:
// a submission from a non-mirror-target org must create BOTH the original
// conversation (in the submitter's own org) and a mirrored copy (in the
// configured FEEDBACK_MIRROR_ORG_ID org), with the mirrored copy carrying
// enough cross-org context (submitting org, submitter identity, original
// conversation id) for a team member to know exactly where it came from.
func TestSubmitFeedbackMirrorsToConfiguredOrg(t *testing.T) {
	repository := newFakeRepository()
	publisher := &fakePublisher{}
	service := NewService(repository, publisher, WithFeedbackMirrorOrgID("mirror_org"))

	detail, err := service.SubmitFeedback(t.Context(), FeedbackInput{
		OrgID:          "org_1",
		ActorUserID:    "user_1",
		FromName:       "Ada",
		FromEmail:      "ada@example.com",
		BodyText:       "The knowledge tab spinner never resolves.",
		IdempotencyKey: "feedback-mirror-0001",
		PageURL:        "/knowledge",
	})
	if err != nil {
		t.Fatalf("SubmitFeedback() error = %v", err)
	}
	if detail == nil {
		t.Fatal("detail = nil, want the submitter's own-org conversation")
	}

	original, ok := repository.stored["feedback-mirror-0001"]
	if !ok {
		t.Fatal("original event was not stored under the caller's idempotency key")
	}
	if original.Detail.OrgID != "org_1" {
		t.Fatalf("original org = %q, want the submitter's own org_1", original.Detail.OrgID)
	}

	mirrored, ok := repository.stored["feedback-mirror-0001:mirror"]
	if !ok {
		t.Fatal("mirrored event was not stored -- mirroring did not run")
	}
	if mirrored.Detail.OrgID != "mirror_org" {
		t.Fatalf("mirrored org = %q, want the configured mirror_org", mirrored.Detail.OrgID)
	}
	if mirrored.Detail.Provider != FeedbackProvider {
		t.Fatalf("mirrored provider = %q, want %q", mirrored.Detail.Provider, FeedbackProvider)
	}
	for _, want := range []string{"org_1", "Ada", "ada@example.com", original.Detail.ID} {
		if !strings.Contains(mirrored.Message.BodyText, want) {
			t.Fatalf("mirrored body_text = %q, want it to contain %q for cross-org context", mirrored.Message.BodyText, want)
		}
	}
	if !strings.Contains(mirrored.Message.BodyText, "/knowledge") {
		t.Fatalf("mirrored body_text = %q, want the reporting page URL included", mirrored.Message.BodyText)
	}
	if len(mirrored.Detail.Tags) != 2 || mirrored.Detail.Tags[0] != FeedbackTag || mirrored.Detail.Tags[1] != FeedbackMirrorTag {
		t.Fatalf("mirrored tags = %#v, want exactly [%q, %q]", mirrored.Detail.Tags, FeedbackTag, FeedbackMirrorTag)
	}
	// The submitter's own-org conversation must stay untouched by the mirror
	// tag -- only the mirrored copy is marked as cross-org.
	if len(original.Detail.Tags) != 1 || original.Detail.Tags[0] != FeedbackTag {
		t.Fatalf("original tags = %#v, want exactly [%q], no mirror tag on the submitter's own copy", original.Detail.Tags, FeedbackTag)
	}
}

// TestSubmitFeedbackSkipsMirrorWhenSubmitterIsMirrorOrg covers team members
// testing their own product: the submitter's org already IS the configured
// mirror target, so mirroring would just create a redundant duplicate.
func TestSubmitFeedbackSkipsMirrorWhenSubmitterIsMirrorOrg(t *testing.T) {
	repository := newFakeRepository()
	service := NewService(repository, nil, WithFeedbackMirrorOrgID("org_1"))

	_, err := service.SubmitFeedback(t.Context(), FeedbackInput{
		OrgID:          "org_1",
		BodyText:       "Testing our own product.",
		IdempotencyKey: "feedback-self-0001",
	})
	if err != nil {
		t.Fatalf("SubmitFeedback() error = %v", err)
	}
	if repository.storeInboundCalls != 1 {
		t.Fatalf("StoreInboundEvent calls = %d, want exactly 1 -- no redundant mirror when the submitter's org already is the mirror target", repository.storeInboundCalls)
	}
	if _, ok := repository.stored["feedback-self-0001:mirror"]; ok {
		t.Fatal("a mirror event was stored, want none when submitter org == mirror org")
	}
}

// TestSubmitFeedbackSkipsMirrorWhenUnconfigured covers an unset
// FEEDBACK_MIRROR_ORG_ID: the mirror step must be skipped gracefully (not
// error), and the submitter's own-org submission must always succeed
// regardless of mirror configuration.
func TestSubmitFeedbackSkipsMirrorWhenUnconfigured(t *testing.T) {
	repository := newFakeRepository()
	service := NewService(repository, nil)

	detail, err := service.SubmitFeedback(t.Context(), FeedbackInput{
		OrgID:          "org_1",
		BodyText:       "No mirror configured.",
		IdempotencyKey: "feedback-nomirror-0001",
	})
	if err != nil {
		t.Fatalf("SubmitFeedback() error = %v, want mirror-unset to never fail the original submission", err)
	}
	if detail == nil || len(detail.Tags) != 1 || detail.Tags[0] != FeedbackTag {
		t.Fatalf("detail.Tags = %#v, want exactly [%q]", detail, FeedbackTag)
	}
	if repository.storeInboundCalls != 1 {
		t.Fatalf("StoreInboundEvent calls = %d, want exactly 1 when FEEDBACK_MIRROR_ORG_ID is unset", repository.storeInboundCalls)
	}
}

func TestAddMessageDefaultsOutboundAndPublishes(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_1"] = &ChannelThreadRef{
		OrgID: "org_1", ConversationID: "conv_1", Provider: "whatsapp", ConnectionID: "conn_1", ProviderThreadID: "phone:recipient",
	}
	publisher := &fakePublisher{}
	sender := &fakeSender{result: &integration.SendResult{ProviderMessageID: "provider-message-1"}}
	now := time.Date(2026, time.June, 3, 12, 10, 0, 0, time.UTC)
	service := NewService(repository, publisher, WithSender(sender), WithNow(func() time.Time { return now }))

	message, err := service.AddMessage(context.Background(), AddMessageInput{
		OrgID:          "org_1",
		ConversationID: "conv_1",
		ActorUserID:    "user_1",
		BodyText:       "Reply",
		IdempotencyKey: "human-reply-defaults-0001",
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

func TestAddMessageDeliversReplyToChannelBackedConversation(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_wa"] = &ChannelThreadRef{
		OrgID:            "org_1",
		ConversationID:   "conv_wa",
		Provider:         "whatsapp",
		ConnectionID:     "conn_9",
		ProviderThreadID: "1067phone:4790012345",
	}
	sender := &fakeSender{result: &integration.SendResult{ProviderMessageID: "wamid.HBgL123"}}
	publisher := &fakePublisher{}
	service := NewService(repository, publisher, WithSender(sender))

	message, err := service.AddMessage(context.Background(), AddMessageInput{
		OrgID:          "org_1",
		ConversationID: "conv_wa",
		ActorUserID:    "user_1",
		BodyText:       "Hei!",
		IdempotencyKey: "human-reply-whatsapp-0001",
	})
	if err != nil {
		t.Fatalf("AddMessage() error = %v", err)
	}
	// The reply must actually be sent through the integration client...
	if sender.calls != 1 {
		t.Fatalf("sender.calls = %d, want 1 (reply must reach the customer)", sender.calls)
	}
	if sender.lastReq.Provider != "whatsapp" || sender.lastReq.ConnectionID != "conn_9" || sender.lastReq.ProviderThreadID != "1067phone:4790012345" {
		t.Fatalf("send request = %#v, want the resolved whatsapp channel ref", sender.lastReq)
	}
	if sender.lastReq.BodyText != "Hei!" {
		t.Fatalf("send body_text = %q, want the reply text", sender.lastReq.BodyText)
	}
	if sender.lastReq.AuthorizationKind != "human_intent" || sender.lastReq.ApprovalID != "" || sender.lastReq.AuthorizationID == "" || sender.lastReq.ActionID != sender.lastReq.AuthorizationID || sender.lastReq.PayloadSHA256 == "" {
		t.Fatalf("human authorization binding = %#v", sender.lastReq)
	}
	if sender.lastReq.IdempotencyKey != "conversation:human-reply-whatsapp-0001" {
		t.Fatalf("idempotency = %q, want durable human reply contract", sender.lastReq.IdempotencyKey)
	}
	// ...and the stored message must record the real delivery.
	if repository.addMessageCalls != 1 {
		t.Fatalf("repository.AddMessage calls = %d, want 1", repository.addMessageCalls)
	}
	if repository.lastMessage.Provider != "whatsapp" || repository.lastMessage.ProviderMessageID != "wamid.HBgL123" {
		t.Fatalf("stored provider/message id = %q/%q, want whatsapp/wamid.HBgL123", repository.lastMessage.Provider, repository.lastMessage.ProviderMessageID)
	}
	if message.ProviderMessageID != "wamid.HBgL123" {
		t.Fatalf("returned message provider_message_id = %q, want wamid.HBgL123", message.ProviderMessageID)
	}
	if len(publisher.subjects) != 1 || publisher.subjects[0] != SubjectMessageSent {
		t.Fatalf("subjects = %#v, want message.sent", publisher.subjects)
	}
}

func TestAddMessageSurfacesSendFailureWithoutStoring(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_wa"] = &ChannelThreadRef{
		OrgID:            "org_1",
		ConversationID:   "conv_wa",
		Provider:         "messenger",
		ConnectionID:     "conn_9",
		ProviderThreadID: "1094page:73940psid",
	}
	sender := &fakeSender{err: &integration.SendError{Terminal: true, Code: "invalid_body", Message: "bad"}}
	publisher := &fakePublisher{}
	service := NewService(repository, publisher, WithSender(sender))

	_, err := service.AddMessage(context.Background(), AddMessageInput{
		OrgID:          "org_1",
		ConversationID: "conv_wa",
		ActorUserID:    "user_1",
		BodyText:       "Takk!",
		IdempotencyKey: "human-reply-failure-0001",
	})
	if !errors.Is(err, ErrSendFailed) {
		t.Fatalf("error = %v, want ErrSendFailed (a failed send must surface, not a false success)", err)
	}
	if sender.calls != 1 {
		t.Fatalf("sender.calls = %d, want 1", sender.calls)
	}
	// No phantom "sent" row may be persisted for a message the customer never got.
	if repository.addMessageCalls != 0 {
		t.Fatalf("repository.AddMessage calls = %d, want 0 (no stored reply on send failure)", repository.addMessageCalls)
	}
	if len(publisher.subjects) != 0 {
		t.Fatalf("subjects = %#v, want none (nothing sent, nothing published)", publisher.subjects)
	}
}

func TestAddMessageProviderAcceptedThenFinalizeFailsRetryDoesNotResend(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_wa"] = &ChannelThreadRef{
		OrgID: "org_1", ConversationID: "conv_wa", Provider: "whatsapp", ConnectionID: "conn_1", ProviderThreadID: "phone-1:recipient-1",
	}
	repository.addMessageErr = errors.New("database unavailable after provider acceptance")
	sender := &fakeSender{result: &integration.SendResult{ProviderMessageID: "wamid.accepted"}}
	service := NewService(repository, &fakePublisher{}, WithSender(sender))
	input := AddMessageInput{
		OrgID: "org_1", ConversationID: "conv_wa", ActorUserID: "user_1", BodyText: "One durable reply", IdempotencyKey: "human-reply-finalize-0001",
	}

	for attempt := 1; attempt <= 2; attempt++ {
		if _, err := service.AddMessage(t.Context(), input); !errors.Is(err, ErrDeliveryUnknown) {
			t.Fatalf("attempt %d error = %v, want ErrDeliveryUnknown", attempt, err)
		}
	}
	if got := sender.count(); got != 1 {
		t.Fatalf("provider sends = %d, want 1 after ambiguous local finalization", got)
	}
}

func TestAddMessageSuccessfulReplayReturnsSameMessageWithoutResend(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_wa"] = &ChannelThreadRef{
		OrgID: "org_1", ConversationID: "conv_wa", Provider: "whatsapp", ConnectionID: "conn_1", ProviderThreadID: "phone-1:recipient-1",
	}
	sender := &fakeSender{result: &integration.SendResult{ProviderMessageID: "wamid.once"}}
	publisher := &fakePublisher{}
	service := NewService(repository, publisher, WithSender(sender))
	input := AddMessageInput{
		OrgID: "org_1", ConversationID: "conv_wa", ActorUserID: "user_1", BodyText: "One durable reply", IdempotencyKey: "human-reply-replay-0001",
	}

	first, err := service.AddMessage(t.Context(), input)
	if err != nil {
		t.Fatal(err)
	}
	second, err := service.AddMessage(t.Context(), input)
	if err != nil {
		t.Fatal(err)
	}
	if first.ID != second.ID || first.ProviderMessageID != second.ProviderMessageID {
		t.Fatalf("replay = %#v, want original %#v", second, first)
	}
	if got := sender.count(); got != 1 {
		t.Fatalf("provider sends = %d, want 1", got)
	}
	if len(publisher.subjects) != 1 {
		t.Fatalf("message.sent publications = %d, want 1", len(publisher.subjects))
	}
}

func TestAddMessageSameKeyDifferentPayloadConflictsWithoutResend(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_wa"] = &ChannelThreadRef{
		OrgID: "org_1", ConversationID: "conv_wa", Provider: "whatsapp", ConnectionID: "conn_1", ProviderThreadID: "phone-1:recipient-1",
	}
	sender := &fakeSender{result: &integration.SendResult{ProviderMessageID: "wamid.once"}}
	service := NewService(repository, &fakePublisher{}, WithSender(sender))
	input := AddMessageInput{
		OrgID: "org_1", ConversationID: "conv_wa", ActorUserID: "user_1", BodyText: "Original reply", IdempotencyKey: "human-reply-conflict-0001",
	}
	if _, err := service.AddMessage(t.Context(), input); err != nil {
		t.Fatal(err)
	}
	input.BodyText = "Changed reply"
	if _, err := service.AddMessage(t.Context(), input); !errors.Is(err, ErrConflict) {
		t.Fatalf("changed replay error = %v, want ErrConflict", err)
	}
	if got := sender.count(); got != 1 {
		t.Fatalf("provider sends = %d, want 1", got)
	}
}

func TestAddMessageConcurrentSameKeySendsOnce(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_wa"] = &ChannelThreadRef{
		OrgID: "org_1", ConversationID: "conv_wa", Provider: "whatsapp", ConnectionID: "conn_1", ProviderThreadID: "phone-1:recipient-1",
	}
	sender := &fakeSender{result: &integration.SendResult{ProviderMessageID: "wamid.once"}}
	service := NewService(repository, &fakePublisher{}, WithSender(sender))
	input := AddMessageInput{
		OrgID: "org_1", ConversationID: "conv_wa", ActorUserID: "user_1", BodyText: "Concurrent reply", IdempotencyKey: "human-reply-concurrent-0001",
	}

	const attempts = 16
	var wg sync.WaitGroup
	wg.Add(attempts)
	for range attempts {
		go func() {
			defer wg.Done()
			_, _ = service.AddMessage(t.Context(), input)
		}()
	}
	wg.Wait()
	if got := sender.count(); got != 1 {
		t.Fatalf("provider sends = %d, want exactly 1", got)
	}
	if _, err := service.AddMessage(t.Context(), input); err != nil {
		t.Fatalf("settled replay error = %v", err)
	}
}

func TestAddMessageTransientProviderOutcomeBecomesDurableUnknown(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_wa"] = &ChannelThreadRef{
		OrgID: "org_1", ConversationID: "conv_wa", Provider: "whatsapp", ConnectionID: "conn_1", ProviderThreadID: "phone-1:recipient-1",
	}
	sender := &fakeSender{err: &integration.SendError{Terminal: false, Code: "transport", Message: "response lost"}}
	service := NewService(repository, &fakePublisher{}, WithSender(sender))
	input := AddMessageInput{
		OrgID: "org_1", ConversationID: "conv_wa", ActorUserID: "user_1", BodyText: "Ambiguous reply", IdempotencyKey: "human-reply-unknown-0001",
	}

	for attempt := 1; attempt <= 2; attempt++ {
		if _, err := service.AddMessage(t.Context(), input); !errors.Is(err, ErrDeliveryUnknown) {
			t.Fatalf("attempt %d error = %v, want ErrDeliveryUnknown", attempt, err)
		}
	}
	if got := sender.count(); got != 1 {
		t.Fatalf("provider sends = %d, want 1 while outcome is unknown", got)
	}
}

func TestAddMessagePreProviderFailureReclaimsSameIntentSafely(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_wa"] = &ChannelThreadRef{
		OrgID: "org_1", ConversationID: "conv_wa", Provider: "whatsapp", ConnectionID: "conn_1", ProviderThreadID: "phone-1:recipient-1",
	}
	sender := &fakeSender{err: &integration.SendError{SafeToRetry: true, Code: "auth_token_unavailable", Message: "pre-provider"}}
	service := NewService(repository, &fakePublisher{}, WithSender(sender))
	input := AddMessageInput{
		OrgID: "org_1", ConversationID: "conv_wa", ActorUserID: "user_1", BodyText: "Safe retry", IdempotencyKey: "human-reply-safe-retry-0001",
	}
	if _, err := service.AddMessage(t.Context(), input); !errors.Is(err, ErrSendFailed) {
		t.Fatalf("first error = %v, want visible ErrSendFailed", err)
	}
	intent := repository.outboundIntents["org_1:"+input.IdempotencyKey]
	if intent == nil || intent.Status != OutboundIntentRetryable {
		t.Fatalf("intent = %#v, want retryable", intent)
	}
	sender.mu.Lock()
	sender.err = nil
	sender.result = &integration.SendResult{ProviderMessageID: "wamid.retry"}
	sender.mu.Unlock()
	if _, err := service.AddMessage(t.Context(), input); err != nil {
		t.Fatalf("same-key retry = %v", err)
	}
	if sender.count() != 2 {
		t.Fatalf("send calls = %d, want pre-provider failure plus one safe retry", sender.count())
	}
}

func TestAddMessageRealHTTPPreProviderSentinelResignsFreshJTIAndSucceeds(t *testing.T) {
	authServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]string{
			"token": "tenant-token", "expiresAt": "2099-01-01T00:00:00Z",
		})
	}))
	defer authServer.Close()
	var attestations []string
	actionServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			WriteAttestation string `json:"writeAttestation"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error(err)
			return
		}
		attestations = append(attestations, body.WriteAttestation)
		if len(attestations) == 1 {
			w.WriteHeader(http.StatusServiceUnavailable)
			_, _ = w.Write([]byte(`{"error":{"code":"action_pre_provider_retryable"}}`))
			return
		}
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"ts":"42.1"}}}}`))
	}))
	defer actionServer.Close()
	privateKey := ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))
	signer, err := attestation.NewSigner(attestation.Config{
		PrivateKey: privateKey, KeyID: "test-key", Issuer: attestation.IssuerConversationCore,
		Audience: attestation.AudienceIntegrationCore, Presenter: attestation.PresenterConversationCore,
		Now:    func() time.Time { return time.Date(2026, time.July, 13, 18, 0, 0, 0, time.UTC) },
		Random: bytes.NewReader(append(bytes.Repeat([]byte{1}, 16), bytes.Repeat([]byte{2}, 16)...)),
	})
	if err != nil {
		t.Fatal(err)
	}
	client := integration.NewClient(actionServer.URL, "internal",
		integration.WithServicePrincipal(authServer.URL, "conversation-core", "credential"),
		integration.WithWriteAttestor(signer),
	)
	repository := newFakeRepository()
	repository.threadRefs["conv_slack"] = &ChannelThreadRef{
		OrgID: "org_1", ConversationID: "conv_slack", Provider: "slack", ConnectionID: "conn_1", ProviderThreadID: "C123",
	}
	service := NewService(repository, &fakePublisher{}, WithSender(client))
	input := AddMessageInput{
		OrgID: "org_1", ConversationID: "conv_slack", ActorUserID: "user_1", BodyText: "Safe retry", IdempotencyKey: "human-reply-http-retry-0001",
	}
	if _, err := service.AddMessage(t.Context(), input); !errors.Is(err, ErrSendFailed) {
		t.Fatalf("first error = %v", err)
	}
	intent := repository.outboundIntents["org_1:"+input.IdempotencyKey]
	if intent == nil || intent.Status != OutboundIntentRetryable {
		t.Fatalf("first intent = %#v", intent)
	}
	if _, err := service.AddMessage(t.Context(), input); err != nil {
		t.Fatalf("same-intent retry = %v", err)
	}
	if len(attestations) != 2 || attestations[0] == attestations[1] {
		t.Fatalf("attestations = %#v, want two fresh proofs", attestations)
	}
	firstClaims := decodeTestAttestationClaims(t, attestations[0])
	secondClaims := decodeTestAttestationClaims(t, attestations[1])
	wantAuthorizationID := OutboundIntentID(input.OrgID, input.IdempotencyKey)
	if firstClaims.AuthorizationID != wantAuthorizationID || secondClaims.AuthorizationID != wantAuthorizationID || firstClaims.PayloadSHA256 != secondClaims.PayloadSHA256 {
		t.Fatalf("claim bindings = %#v / %#v", firstClaims, secondClaims)
	}
	if firstClaims.JWTID == secondClaims.JWTID {
		t.Fatal("safe retry reused the attestation jti")
	}
	for _, jti := range []string{firstClaims.JWTID, secondClaims.JWTID} {
		decoded, err := base64.RawURLEncoding.DecodeString(jti)
		if err != nil || len(decoded) != 16 {
			t.Fatalf("jti %q = %d bytes, %v", jti, len(decoded), err)
		}
	}
}

func decodeTestAttestationClaims(t *testing.T, compact string) attestation.Claims {
	t.Helper()
	parts := strings.Split(compact, ".")
	if len(parts) != 3 {
		t.Fatalf("compact JWS parts = %d", len(parts))
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		t.Fatal(err)
	}
	var claims attestation.Claims
	if err := json.Unmarshal(payload, &claims); err != nil {
		t.Fatal(err)
	}
	return claims
}

func TestAddMessageInternalNoteDoesNotSend(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_wa"] = &ChannelThreadRef{
		OrgID: "org_1", ConversationID: "conv_wa", Provider: "whatsapp", ConnectionID: "conn_9", ProviderThreadID: "b:r",
	}
	sender := &fakeSender{}
	service := NewService(repository, &fakePublisher{}, WithSender(sender))

	if _, err := service.AddMessage(context.Background(), AddMessageInput{
		OrgID:          "org_1",
		ConversationID: "conv_wa",
		ActorUserID:    "user_1",
		BodyText:       "internal note",
		Internal:       true,
	}); err != nil {
		t.Fatalf("AddMessage(note) error = %v", err)
	}
	if sender.calls != 0 {
		t.Fatalf("sender.calls = %d, want 0 (internal notes are never sent to the customer)", sender.calls)
	}
	if repository.addMessageCalls != 1 {
		t.Fatalf("repository.AddMessage calls = %d, want 1 (note is still stored)", repository.addMessageCalls)
	}
}

func TestAddMessageExternalReplyRequiresIdempotencyKeyBeforeSend(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_wa"] = &ChannelThreadRef{
		OrgID: "org_1", ConversationID: "conv_wa", Provider: "whatsapp", ConnectionID: "conn_9", ProviderThreadID: "b:r",
	}
	sender := &fakeSender{}
	service := NewService(repository, &fakePublisher{}, WithSender(sender))

	_, err := service.AddMessage(t.Context(), AddMessageInput{
		OrgID:          "org_1",
		ConversationID: "conv_wa",
		ActorUserID:    "user_1",
		BodyText:       "Reply",
	})
	if !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("AddMessage() error = %v, want ErrInvalidInput", err)
	}
	if sender.calls != 0 {
		t.Fatalf("sender.calls = %d, want 0 before idempotency validation", sender.calls)
	}
}

func TestAddMessageWithoutChannelRefFailsWithoutStoring(t *testing.T) {
	repository := newFakeRepository()
	sender := &fakeSender{}
	service := NewService(repository, &fakePublisher{}, WithSender(sender))

	if _, err := service.AddMessage(context.Background(), AddMessageInput{
		OrgID:          "org_1",
		ConversationID: "conv_email",
		ActorUserID:    "user_1",
		BodyText:       "Reply",
		IdempotencyKey: "human-reply-no-ref-0001",
	}); !errors.Is(err, ErrDeliveryUnavailable) {
		t.Fatalf("AddMessage() error = %v, want ErrDeliveryUnavailable", err)
	}
	if sender.calls != 0 {
		t.Fatalf("sender.calls = %d, want 0 (no channel ref => store only)", sender.calls)
	}
	if repository.addMessageCalls != 0 {
		t.Fatalf("repository.AddMessage calls = %d, want 0", repository.addMessageCalls)
	}
}

func TestAddMessageUnsupportedProviderFailsWithoutStoring(t *testing.T) {
	repository := newFakeRepository()
	repository.threadRefs["conv_x"] = &ChannelThreadRef{
		OrgID: "org_1", ConversationID: "conv_x", Provider: "discord", ConnectionID: "c", ProviderThreadID: "t",
	}
	sender := &fakeSender{}
	service := NewService(repository, &fakePublisher{}, WithSender(sender))

	if _, err := service.AddMessage(context.Background(), AddMessageInput{
		OrgID:          "org_1",
		ConversationID: "conv_x",
		ActorUserID:    "user_1",
		BodyText:       "Reply",
		IdempotencyKey: "human-reply-unsupported-0001",
	}); !errors.Is(err, ErrDeliveryUnavailable) {
		t.Fatalf("AddMessage() error = %v, want ErrDeliveryUnavailable", err)
	}
	if sender.calls != 0 {
		t.Fatalf("sender.calls = %d, want 0 (unsupported provider => store only, no false error)", sender.calls)
	}
	if repository.addMessageCalls != 0 {
		t.Fatalf("repository.AddMessage calls = %d, want 0", repository.addMessageCalls)
	}
}

func TestAddMessageExternalMissingSenderFailsWithoutStoring(t *testing.T) {
	repository := newFakeRepository()
	service := NewService(repository, &fakePublisher{})

	if _, err := service.AddMessage(context.Background(), AddMessageInput{
		OrgID:          "org_1",
		ConversationID: "conv_external",
		ActorUserID:    "user_1",
		BodyText:       "Reply",
		IdempotencyKey: "human-reply-no-sender-0001",
	}); !errors.Is(err, ErrDeliveryUnavailable) {
		t.Fatalf("AddMessage() error = %v, want ErrDeliveryUnavailable", err)
	}
	if repository.addMessageCalls != 0 {
		t.Fatalf("repository.AddMessage calls = %d, want 0", repository.addMessageCalls)
	}
}

func TestRecordTicketClassificationCreatesSuggestedTicket(t *testing.T) {
	repository := newFakeRepository()
	repository.details["conv_1"] = &ConversationDetail{ConversationSummary: ConversationSummary{ID: "conv_1", OrgID: "org_1"}}
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	classification, err := service.RecordTicketClassification(context.Background(), TicketClassificationInput{
		OrgID:          "org_1",
		ConversationID: "conv_1",
		Outcome:        "suggest_ticket",
		Confidence:     0.82,
		Reason:         "Customer asks for a refund and needs owner follow-up.",
		SuggestedFields: map[string]any{
			"category": "refund",
			"priority": "high",
		},
		EvidenceMessageIDs: []string{"msg_1"},
	})
	if err != nil {
		t.Fatalf("RecordTicketClassification() error = %v", err)
	}
	if classification.Outcome != "suggest_ticket" {
		t.Fatalf("outcome = %q, want suggest_ticket", classification.Outcome)
	}
	if classification.Ticket == nil || classification.Ticket.Status != "suggested" {
		t.Fatalf("ticket = %#v, want suggested ticket", classification.Ticket)
	}
	if len(publisher.subjects) != 1 || publisher.subjects[0] != SubjectTicketSuggested {
		t.Fatalf("subjects = %#v, want ticket suggested", publisher.subjects)
	}
}

func TestRecordTicketClassificationDoesNotAutoCreateSensitiveCategory(t *testing.T) {
	repository := newFakeRepository()
	repository.details["conv_1"] = &ConversationDetail{ConversationSummary: ConversationSummary{ID: "conv_1", OrgID: "org_1"}}
	service := NewService(repository, nil)

	classification, err := service.RecordTicketClassification(context.Background(), TicketClassificationInput{
		OrgID:          "org_1",
		ConversationID: "conv_1",
		Outcome:        "auto_ticket",
		Confidence:     0.98,
		Reason:         "Sensitive payment dispute requires review.",
		SuggestedFields: map[string]any{
			"category": "payment dispute",
			"priority": "high",
		},
		EvidenceMessageIDs: []string{"msg_1"},
	})
	if err != nil {
		t.Fatalf("RecordTicketClassification() error = %v", err)
	}
	if classification.Outcome != "suggest_ticket" {
		t.Fatalf("outcome = %q, want suggest_ticket", classification.Outcome)
	}
	if classification.Ticket == nil || classification.Ticket.Status != "suggested" {
		t.Fatalf("ticket = %#v, want suggested ticket", classification.Ticket)
	}
}

func TestRunTicketMacroAppliesOperationalActions(t *testing.T) {
	repository := newFakeRepository()
	repository.tickets["ticket_1"] = &Ticket{
		ID:             "ticket_1",
		OrgID:          "org_1",
		ConversationID: "conv_1",
		TicketKey:      "TCK-FAKE",
		Status:         "open",
		Priority:       "normal",
		Severity:       "medium",
		Labels:         []string{},
	}
	repository.macros["macro_1"] = &TicketMacro{
		ID:     "macro_1",
		OrgID:  "org_1",
		Name:   "Refund handoff",
		Active: true,
		Actions: map[string]any{
			"status":   "waiting_team",
			"priority": "high",
			"team_id":  "billing",
			"labels":   []any{"refund", "handoff"},
		},
	}
	service := NewService(repository, nil)

	result, err := service.RunTicketMacro(context.Background(), TicketMacroRunInput{
		OrgID:       "org_1",
		TicketID:    "ticket_1",
		MacroID:     "macro_1",
		ActorUserID: "user_1",
	})
	if err != nil {
		t.Fatalf("RunTicketMacro() error = %v", err)
	}
	if result.Ticket.Status != "waiting_team" {
		t.Fatalf("status = %q, want waiting_team", result.Ticket.Status)
	}
	if result.Ticket.Priority != "high" || result.Ticket.TeamID != "billing" {
		t.Fatalf("ticket = %#v, want macro priority/team", result.Ticket)
	}
	if len(result.Ticket.Labels) != 2 || result.Ticket.Labels[0] != "refund" {
		t.Fatalf("labels = %#v, want refund/handoff", result.Ticket.Labels)
	}
}

func TestCreateTicketEvaluatesAutomationRule(t *testing.T) {
	repository := newFakeRepository()
	repository.details["conv_1"] = &ConversationDetail{ConversationSummary: ConversationSummary{ID: "conv_1", OrgID: "org_1"}}
	repository.rules = []TicketAutomationRule{{
		ID:        "rule_1",
		OrgID:     "org_1",
		Name:      "Route refunds",
		EventName: "ticket.created",
		Active:    true,
		Conditions: map[string]any{
			"category": "refund",
		},
		Actions: map[string]any{
			"status":    "waiting_team",
			"team_id":   "billing",
			"team_name": "Billing",
			"labels":    []any{"refund", "automation"},
		},
	}}
	service := NewService(repository, nil)

	ticket, err := service.CreateTicket(context.Background(), CreateTicketInput{
		OrgID:          "org_1",
		ConversationID: "conv_1",
		Category:       "refund",
		ActorUserID:    "user_1",
	})
	if err != nil {
		t.Fatalf("CreateTicket() error = %v", err)
	}
	if ticket.Status != "waiting_team" || ticket.TeamID != "billing" {
		t.Fatalf("ticket = %#v, want automation-routed billing ticket", ticket)
	}
	if len(ticket.Labels) != 2 || ticket.Labels[1] != "automation" {
		t.Fatalf("labels = %#v, want refund/automation", ticket.Labels)
	}
}

func TestUpdateTicketPublishesAssignedAndResolvedLifecycleEvents(t *testing.T) {
	repository := newFakeRepository()
	repository.tickets["ticket_1"] = &Ticket{
		ID:             "ticket_1",
		OrgID:          "org_1",
		ConversationID: "conv_1",
		TicketKey:      "TCK-FAKE",
		Status:         "open",
		Priority:       "normal",
		Severity:       "medium",
	}
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	assigneeID := "user_2"
	if _, err := service.UpdateTicket(context.Background(), UpdateTicketInput{
		OrgID:          "org_1",
		TicketID:       "ticket_1",
		ActorUserID:    "user_1",
		AssigneeUserID: &assigneeID,
	}); err != nil {
		t.Fatalf("UpdateTicket(assign) error = %v", err)
	}

	resolved := "resolved"
	if _, err := service.UpdateTicket(context.Background(), UpdateTicketInput{
		OrgID:       "org_1",
		TicketID:    "ticket_1",
		ActorUserID: "user_1",
		Status:      &resolved,
	}); err != nil {
		t.Fatalf("UpdateTicket(resolve) error = %v", err)
	}

	if len(publisher.subjects) != 2 {
		t.Fatalf("subjects = %#v, want assigned then resolved", publisher.subjects)
	}
	if publisher.subjects[0] != SubjectTicketAssigned || publisher.subjects[1] != SubjectTicketResolved {
		t.Fatalf("subjects = %#v, want assigned then resolved", publisher.subjects)
	}
}

func TestTicketChecklistItemCompletion(t *testing.T) {
	repository := newFakeRepository()
	service := NewService(repository, nil)

	checklist, err := service.CreateTicketChecklist(context.Background(), CreateTicketChecklistInput{
		OrgID:           "org_1",
		TicketID:        "ticket_1",
		Name:            "Refund review",
		Items:           []string{"Confirm refund", "Notify customer"},
		CreatedByUserID: "user_1",
	})
	if err != nil {
		t.Fatalf("CreateTicketChecklist() error = %v", err)
	}
	if len(checklist.Items) != 2 {
		t.Fatalf("items = %#v, want two checklist items", checklist.Items)
	}

	updated, err := service.UpdateTicketChecklistItem(context.Background(), UpdateTicketChecklistItemInput{
		OrgID:       "org_1",
		TicketID:    "ticket_1",
		ChecklistID: checklist.ID,
		ItemID:      checklist.Items[0].ID,
		Completed:   true,
		ActorUserID: "user_1",
	})
	if err != nil {
		t.Fatalf("UpdateTicketChecklistItem() error = %v", err)
	}
	if !updated.Items[0].Completed {
		t.Fatal("first checklist item is not completed")
	}
}

func TestReviewAIActionRejectsUnknownDecision(t *testing.T) {
	for _, decision := range []string{"maybe", "approve", "APPROVED", "deleted", "ok"} {
		repository := newFakeRepository()
		service := NewService(repository, &fakePublisher{})
		err := service.ReviewAIAction(context.Background(), AIActionReview{
			OrgID:      "org_1",
			AIActionID: "aiact_1",
			ReviewerID: "user_1",
			Decision:   decision,
		})
		if !errors.Is(err, ErrInvalidInput) {
			t.Fatalf("decision %q: error = %v, want ErrInvalidInput", decision, err)
		}
		if repository.reviewCalls != 0 {
			t.Fatalf("decision %q: repository.ReviewAIAction called %d times, want 0 (rejected before reaching the repository)", decision, repository.reviewCalls)
		}
	}
}

func TestReviewAIActionRequiresOrg(t *testing.T) {
	repository := newFakeRepository()
	service := NewService(repository, &fakePublisher{})
	err := service.ReviewAIAction(context.Background(), AIActionReview{
		AIActionID: "aiact_1",
		ReviewerID: "user_1",
		Decision:   "approved",
	})
	if !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("error = %v, want ErrInvalidInput", err)
	}
	if repository.reviewCalls != 0 {
		t.Fatalf("repository.ReviewAIAction called %d times, want 0", repository.reviewCalls)
	}
}

func TestReviewAIActionPropagatesNotFoundWithoutPublishing(t *testing.T) {
	repository := newFakeRepository()
	repository.reviewErr = ErrNotFound
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	err := service.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:      "org_1",
		AIActionID: "missing",
		ReviewerID: "user_1",
		Decision:   "approved",
	})
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("error = %v, want ErrNotFound", err)
	}
	if len(publisher.subjects) != 0 {
		t.Fatalf("subjects = %#v, want none (no reviewed event for a non-existent action)", publisher.subjects)
	}
}

func TestReviewAIActionPropagatesConflictWithoutPublishing(t *testing.T) {
	repository := newFakeRepository()
	repository.reviewErr = ErrConflict
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	err := service.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:      "org_1",
		AIActionID: "aiact_1",
		ReviewerID: "user_1",
		Decision:   "approved",
	})
	if !errors.Is(err, ErrConflict) {
		t.Fatalf("error = %v, want ErrConflict", err)
	}
	if len(publisher.subjects) != 0 {
		t.Fatalf("subjects = %#v, want none (no reviewed event for a terminal action)", publisher.subjects)
	}
}

func TestReviewAIActionValidDecisionsPublishReviewed(t *testing.T) {
	for _, decision := range []string{"approved", "rejected"} {
		repository := newFakeRepository()
		publisher := &fakePublisher{}
		service := NewService(repository, publisher)

		err := service.ReviewAIAction(context.Background(), AIActionReview{
			OrgID:      "org_1",
			AIActionID: "aiact_1",
			ReviewerID: "user_1",
			Decision:   decision,
		})
		if err != nil {
			t.Fatalf("decision %q: ReviewAIAction() error = %v", decision, err)
		}
		if repository.reviewCalls != 1 || repository.lastReview.Decision != decision {
			t.Fatalf("decision %q: repository review = (calls %d, decision %q), want (1, %q)", decision, repository.reviewCalls, repository.lastReview.Decision, decision)
		}
		if len(publisher.subjects) != 1 || publisher.subjects[0] != SubjectAIActionReviewed {
			t.Fatalf("decision %q: subjects = %#v, want ai_action.reviewed once", decision, publisher.subjects)
		}
	}
}

// Approving with whitelisted edited fields threads exactly the trimmed,
// non-empty values through to the repository for the atomic merge.
func TestReviewAIActionThreadsWhitelistedEditedFieldsOnApprove(t *testing.T) {
	repository := newFakeRepository()
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	err := service.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:      "org_1",
		AIActionID: "aiact_1",
		ReviewerID: "user_1",
		Decision:   "approved",
		EditedFields: map[string]string{
			"category": "  sales  ",
			"priority": "",
			"intent":   "refund",
		},
	})
	if err != nil {
		t.Fatalf("ReviewAIAction() error = %v, want nil", err)
	}
	want := map[string]string{"category": "sales", "intent": "refund"}
	if len(repository.lastReview.EditedFields) != len(want) {
		t.Fatalf("lastReview.EditedFields = %#v, want %#v", repository.lastReview.EditedFields, want)
	}
	for key, value := range want {
		if repository.lastReview.EditedFields[key] != value {
			t.Fatalf("lastReview.EditedFields[%q] = %q, want %q", key, repository.lastReview.EditedFields[key], value)
		}
	}
}

// A non-whitelisted key in edited fields (anything outside promote()'s actual
// reads: category, priority, severity, intent, team_id, team_name) must be
// silently dropped -- never applied, but never failing the whole request --
// mirroring the JSON-decode posture where an unrecognized field is simply not
// bound.
func TestReviewAIActionDropsNonWhitelistedEditedField(t *testing.T) {
	repository := newFakeRepository()
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	err := service.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:      "org_1",
		AIActionID: "aiact_1",
		ReviewerID: "user_1",
		Decision:   "approved",
		EditedFields: map[string]string{
			"category":     "sales",
			"assigned_bot": "sneaky-value",
		},
	})
	if err != nil {
		t.Fatalf("ReviewAIAction() error = %v, want nil", err)
	}
	if repository.reviewCalls != 1 {
		t.Fatalf("repository.ReviewAIAction called %d times, want 1", repository.reviewCalls)
	}
	if _, ok := repository.lastReview.EditedFields["assigned_bot"]; ok {
		t.Fatalf("lastReview.EditedFields = %#v, want assigned_bot dropped", repository.lastReview.EditedFields)
	}
	if got := repository.lastReview.EditedFields["category"]; got != "sales" {
		t.Fatalf("lastReview.EditedFields[category] = %q, want sales", got)
	}
}

// The reject path must never thread edited fields through to the repository,
// even if a caller (contrary to the frontend's own contract) attached them --
// approve and reject are asymmetric here by design.
func TestReviewAIActionRejectNeverThreadsEditedFields(t *testing.T) {
	repository := newFakeRepository()
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	err := service.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:        "org_1",
		AIActionID:   "aiact_1",
		ReviewerID:   "user_1",
		Decision:     "rejected",
		EditedFields: map[string]string{"category": "sales"},
	})
	if err != nil {
		t.Fatalf("ReviewAIAction() error = %v, want nil", err)
	}
	if len(repository.lastReview.EditedFields) != 0 {
		t.Fatalf("lastReview.EditedFields = %#v, want none for a reject", repository.lastReview.EditedFields)
	}
}

// Approving with no edited fields must behave exactly as before this feature
// existed: EditedFields reaching the repository is empty, not merely "some
// keys quietly filtered out".
func TestReviewAIActionApproveWithoutEditedFieldsIsUnchanged(t *testing.T) {
	repository := newFakeRepository()
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	err := service.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:      "org_1",
		AIActionID: "aiact_1",
		ReviewerID: "user_1",
		Decision:   "approved",
	})
	if err != nil {
		t.Fatalf("ReviewAIAction() error = %v, want nil", err)
	}
	if len(repository.lastReview.EditedFields) != 0 {
		t.Fatalf("lastReview.EditedFields = %#v, want none", repository.lastReview.EditedFields)
	}
	if repository.reviewCalls != 1 || repository.lastReview.Decision != "approved" {
		t.Fatalf("repository review = (calls %d, decision %q), want (1, approved)", repository.reviewCalls, repository.lastReview.Decision)
	}
}

func TestListAIActionsDefaultsToSuggestedAndScopesByOrg(t *testing.T) {
	repository := newFakeRepository()
	repository.aiActions = []AIAction{
		{ID: "a1", OrgID: "org_1", Status: "suggested", ConversationID: "conv_1"},
		{ID: "a2", OrgID: "org_1", Status: "approved", ConversationID: "conv_1"},
		{ID: "a3", OrgID: "org_2", Status: "suggested", ConversationID: "conv_9"},
	}
	service := NewService(repository, nil)

	// Default (no status) ⇒ only suggested actions for the caller's org.
	got, err := service.ListAIActions(context.Background(), AIActionListFilter{OrgID: "org_1"})
	if err != nil {
		t.Fatalf("ListAIActions() error = %v", err)
	}
	if len(got) != 1 || got[0].ID != "a1" {
		t.Fatalf("default list = %#v, want only suggested action a1 for org_1", got)
	}

	// A foreign org's suggested action (a3) must never leak through.
	for _, action := range got {
		if action.OrgID != "org_1" {
			t.Fatalf("leaked foreign-org action: %#v", action)
		}
	}

	// status=all ⇒ every status for org_1 (a1 + a2), still org-scoped.
	all, err := service.ListAIActions(context.Background(), AIActionListFilter{OrgID: "org_1", Status: "all"})
	if err != nil {
		t.Fatalf("ListAIActions(all) error = %v", err)
	}
	if len(all) != 2 {
		t.Fatalf("status=all list = %#v, want both org_1 actions", all)
	}
}

func TestListAIActionsRequiresOrg(t *testing.T) {
	service := NewService(newFakeRepository(), nil)
	_, err := service.ListAIActions(context.Background(), AIActionListFilter{})
	if !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("error = %v, want ErrInvalidInput", err)
	}
}

// A stuck-send sweep must forward the exact staleness threshold to the
// repository and, for every intent it reconciles, publish the same
// SubjectAIActionSendUnknown alert the live send path emits for an ambiguous
// outcome — so an operator watching that one subject sees both cases.
func TestReconcileStaleOutboundIntentsPublishesUnknownForEachReconciledIntent(t *testing.T) {
	repository := newFakeRepository()
	repository.reconcileResult = []OutboundIntent{
		{
			ID: "oi_1", OrgID: "org_1", ConversationID: "conv_1", AIActionID: "act_1",
			IdempotencyKey: "conversation-ai:act_1", Provider: "microsoft", ActorUserID: "user_1",
		},
		{
			ID: "oi_2", OrgID: "org_1", ConversationID: "conv_2", AIActionID: "act_2",
			IdempotencyKey: "conversation-ai:act_2", Provider: "gmail", ActorUserID: "user_2",
		},
	}
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	reconciled, err := service.ReconcileStaleOutboundIntents(context.Background(), 15*time.Minute)
	if err != nil {
		t.Fatalf("ReconcileStaleOutboundIntents() error = %v", err)
	}
	if len(reconciled) != 2 {
		t.Fatalf("reconciled = %#v, want 2 intents", reconciled)
	}
	if len(repository.reconcileCalls) != 1 || repository.reconcileCalls[0] != 15*time.Minute {
		t.Fatalf("repository staleAfter calls = %v, want [15m]", repository.reconcileCalls)
	}
	if len(publisher.subjects) != 2 {
		t.Fatalf("published subjects = %#v, want 2", publisher.subjects)
	}
	for i, subject := range publisher.subjects {
		if subject != SubjectAIActionSendUnknown {
			t.Fatalf("subject[%d] = %q, want %q", i, subject, SubjectAIActionSendUnknown)
		}
	}
	first := publisher.events[0]
	if first.ConversationID != "conv_1" || first.OrgID != "org_1" {
		t.Fatalf("event[0] org/conversation = %s/%s, want org_1/conv_1", first.OrgID, first.ConversationID)
	}
	if first.Data["ai_action_id"] != "act_1" || first.Data["error_code"] != OutboundIntentErrorStaleSendingTimeout {
		t.Fatalf("event[0] data = %#v, want ai_action_id=act_1 error_code=%s", first.Data, OutboundIntentErrorStaleSendingTimeout)
	}
	if first.Data["status"] != OutboundIntentUnknown {
		t.Fatalf("event[0] status = %v, want unknown", first.Data["status"])
	}
}

// A repository failure must propagate without publishing a false alert for
// intents that were never actually reconciled.
func TestReconcileStaleOutboundIntentsPropagatesRepositoryError(t *testing.T) {
	repository := newFakeRepository()
	repository.reconcileErr = errors.New("db unavailable")
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	_, err := service.ReconcileStaleOutboundIntents(context.Background(), 15*time.Minute)
	if err == nil || !strings.Contains(err.Error(), "db unavailable") {
		t.Fatalf("error = %v, want db unavailable", err)
	}
	if len(publisher.subjects) != 0 {
		t.Fatalf("subjects = %#v, want none published on repository error", publisher.subjects)
	}
}

// No stuck rows is the steady-state common case and must be a silent no-op:
// no publish calls at all.
func TestReconcileStaleOutboundIntentsNoOpWhenNothingStuck(t *testing.T) {
	repository := newFakeRepository()
	publisher := &fakePublisher{}
	service := NewService(repository, publisher)

	reconciled, err := service.ReconcileStaleOutboundIntents(context.Background(), 15*time.Minute)
	if err != nil {
		t.Fatalf("ReconcileStaleOutboundIntents() error = %v", err)
	}
	if len(reconciled) != 0 {
		t.Fatalf("reconciled = %#v, want none", reconciled)
	}
	if len(publisher.subjects) != 0 {
		t.Fatalf("subjects = %#v, want none published", publisher.subjects)
	}
}

func TestHardPurgeByOrgDelegatesTrimmedOrgIDToRepository(t *testing.T) {
	repository := newFakeRepository()
	service := NewService(repository, &fakePublisher{})

	if err := service.HardPurgeByOrg(context.Background(), "  org-1  "); err != nil {
		t.Fatalf("HardPurgeByOrg() error = %v", err)
	}
	if len(repository.hardPurgeCalls) != 1 || repository.hardPurgeCalls[0] != "org-1" {
		t.Fatalf("repository purge calls = %#v, want exactly one trimmed org-1", repository.hardPurgeCalls)
	}
}

func TestHardPurgeByOrgRejectsBlankOrgIDWithoutCallingRepository(t *testing.T) {
	repository := newFakeRepository()
	service := NewService(repository, &fakePublisher{})

	if err := service.HardPurgeByOrg(context.Background(), "   "); !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("error = %v, want ErrInvalidInput", err)
	}
	if len(repository.hardPurgeCalls) != 0 {
		t.Fatalf("repository purge calls = %#v, want none for a blank org_id", repository.hardPurgeCalls)
	}
}

func TestHardPurgeByOrgPropagatesRepositoryError(t *testing.T) {
	repository := newFakeRepository()
	repository.hardPurgeErr = errors.New("db unavailable")
	service := NewService(repository, &fakePublisher{})

	if err := service.HardPurgeByOrg(context.Background(), "org-1"); err == nil || !strings.Contains(err.Error(), "db unavailable") {
		t.Fatalf("error = %v, want db unavailable", err)
	}
}

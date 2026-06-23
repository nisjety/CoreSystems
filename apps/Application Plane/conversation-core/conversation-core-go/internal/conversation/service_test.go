package conversation

import (
	"context"
	"errors"
	"testing"
	"time"
)

type fakeRepository struct {
	inboxes         []Inbox
	conversations   []ConversationSummary
	details         map[string]*ConversationDetail
	stored          map[string]*StoredEventResult
	lastMessage     AddMessageInput
	statusUpdate    StatusUpdate
	tickets         map[string]*Ticket
	macros          map[string]*TicketMacro
	checklists      map[string]*TicketChecklist
	rules           []TicketAutomationRule
	classifications []TicketClassificationInput
	aiActions       []AIAction
	lastReview      AIActionReview
	reviewCalls     int
	reviewErr       error
}

func newFakeRepository() *fakeRepository {
	return &fakeRepository{
		details:    make(map[string]*ConversationDetail),
		stored:     make(map[string]*StoredEventResult),
		tickets:    make(map[string]*Ticket),
		macros:     make(map[string]*TicketMacro),
		checklists: make(map[string]*TicketChecklist),
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

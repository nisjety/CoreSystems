package consumers

import (
	"context"
	"errors"
	"sync"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/integration"
)

// fakeStore models conversation_ai_actions with an atomic approved→executed
// claim guarded by a mutex, so concurrent deliveries race exactly as the SQL
// UPDATE ... WHERE status='approved' does in Postgres.
type fakeStore struct {
	mu           sync.Mutex
	action       *conversation.AIAction
	ticket       *conversation.Ticket
	threadRef    *conversation.ChannelThreadRef
	getErr       error
	getTicketErr error
	getRefErr    error
	claimErr     error

	claimWins    int // claims that observed RowsAffected == 1
	claimCalls   int
	unclaimCalls int
}

func (f *fakeStore) GetAIAction(_ context.Context, orgID, id string) (*conversation.AIAction, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.getErr != nil {
		return nil, f.getErr
	}
	if f.action == nil || f.action.OrgID != orgID || f.action.ID != id {
		return nil, conversation.ErrNotFound
	}
	cp := *f.action
	return &cp, nil
}

func (f *fakeStore) GetTicketByConversation(_ context.Context, orgID, conversationID string) (*conversation.Ticket, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.getTicketErr != nil {
		return nil, f.getTicketErr
	}
	if f.ticket == nil || f.ticket.OrgID != orgID || f.ticket.ConversationID != conversationID {
		return nil, conversation.ErrNotFound
	}
	cp := *f.ticket
	return &cp, nil
}

func (f *fakeStore) GetChannelThreadRefByConversation(_ context.Context, orgID, conversationID string) (*conversation.ChannelThreadRef, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.getRefErr != nil {
		return nil, f.getRefErr
	}
	if f.threadRef == nil || f.threadRef.OrgID != orgID || f.threadRef.ConversationID != conversationID {
		return nil, conversation.ErrNotFound
	}
	cp := *f.threadRef
	return &cp, nil
}

func (f *fakeStore) MarkAIActionExecuted(_ context.Context, orgID, id string) (bool, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.claimCalls++
	if f.claimErr != nil {
		return false, f.claimErr
	}
	if f.action == nil || f.action.OrgID != orgID || f.action.ID != id {
		return false, nil
	}
	if f.action.Status != "approved" {
		return false, nil // already executed → claim loses
	}
	f.action.Status = "executed"
	f.claimWins++
	return true, nil
}

func (f *fakeStore) UnmarkAIActionExecuted(_ context.Context, orgID, id string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.action != nil && f.action.OrgID == orgID && f.action.ID == id && f.action.Status == "executed" {
		f.action.Status = "approved"
		f.unclaimCalls++
	}
	return nil
}

type fakeTickets struct {
	mu      sync.Mutex
	updates []conversation.UpdateTicketInput
	err     error
}

func (f *fakeTickets) UpdateTicket(_ context.Context, input conversation.UpdateTicketInput) (*conversation.Ticket, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.err != nil {
		return nil, f.err
	}
	f.updates = append(f.updates, input)
	return &conversation.Ticket{ID: input.TicketID, OrgID: input.OrgID, Status: "open"}, nil
}

func (f *fakeTickets) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.updates)
}

type fakePublisher struct {
	mu       sync.Mutex
	subjects []string
}

func (f *fakePublisher) Publish(_ context.Context, subject string, _ any) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.subjects = append(f.subjects, subject)
	return nil
}

func (f *fakePublisher) countExecuted() int {
	return f.countSubject(conversation.SubjectAIActionExecuted)
}

func (f *fakePublisher) countSubject(subject string) int {
	f.mu.Lock()
	defer f.mu.Unlock()
	n := 0
	for _, s := range f.subjects {
		if s == subject {
			n++
		}
	}
	return n
}

// fakeSender records send attempts behind a mutex so concurrent deliveries race
// exactly as a real send would, letting the no-double-send test assert exactly
// one Send call survives the atomic claim.
type fakeSender struct {
	mu     sync.Mutex
	calls  int
	result *integration.SendResult
	err    error
}

func (f *fakeSender) Send(_ context.Context, _ integration.SendRequest) (*integration.SendResult, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls++
	if f.err != nil {
		return nil, f.err
	}
	if f.result != nil {
		return f.result, nil
	}
	return &integration.SendResult{ProviderMessageID: "pmid-1", Operation: "message.send"}, nil
}

func (f *fakeSender) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.calls
}

// draftFixture wires an executor over a fresh approved draft.reply action, its
// resolved channel thread ref, and a fakeSender ready to confirm the send.
func draftFixture() (*AIActionExecutor, *fakeStore, *fakeSender, *fakePublisher) {
	store := &fakeStore{
		action: &conversation.AIAction{
			ID:             "act-1",
			OrgID:          "org-1",
			ConversationID: "conv-1",
			Kind:           kindDraftReply,
			Status:         "approved",
			Payload: map[string]any{
				"body_text": "Thanks for reaching out — here is your answer.",
			},
		},
		threadRef: &conversation.ChannelThreadRef{
			OrgID:            "org-1",
			ConversationID:   "conv-1",
			Provider:         "slack",
			ConnectionID:     "conn-1",
			ProviderThreadID: "C123",
		},
	}
	sender := &fakeSender{}
	publisher := &fakePublisher{}
	exec := &AIActionExecutor{store: store, publisher: publisher, sender: sender}
	return exec, store, sender, publisher
}

// fixture wires an executor over a fresh approved ticket.classification action
// and its suggested ticket, ready to execute.
func fixture() (*AIActionExecutor, *fakeStore, *fakeTickets, *fakePublisher) {
	store := &fakeStore{
		action: &conversation.AIAction{
			ID:             "act-1",
			OrgID:          "org-1",
			ConversationID: "conv-1",
			Kind:           kindTicketClassification,
			Status:         "approved",
			Payload: map[string]any{
				"suggested_fields": map[string]any{
					"category": "billing",
					"priority": "high",
					"team_id":  "team-fin",
				},
			},
		},
		ticket: &conversation.Ticket{
			ID:             "tkt-1",
			OrgID:          "org-1",
			ConversationID: "conv-1",
			Status:         "suggested",
		},
	}
	tickets := &fakeTickets{}
	publisher := &fakePublisher{}
	exec := &AIActionExecutor{store: store, tickets: tickets, publisher: publisher}
	return exec, store, tickets, publisher
}

func reviewedEvent(orgID, actionID, decision string) conversation.LifecycleEvent {
	return conversation.LifecycleEvent{
		Type:  "ai_action.reviewed",
		OrgID: orgID,
		Data:  map[string]any{"ai_action_id": actionID, "decision": decision},
	}
}

func TestProcess_ApprovedTicketClassification_PromotesRoutesAndEmits(t *testing.T) {
	exec, store, tickets, pub := fixture()

	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if tickets.count() != 1 {
		t.Fatalf("UpdateTicket called %d times, want 1", tickets.count())
	}
	upd := tickets.updates[0]
	if upd.Status == nil || *upd.Status != "open" {
		t.Errorf("promotion did not set status=open: %+v", upd.Status)
	}
	if upd.Category == nil || *upd.Category != "billing" {
		t.Errorf("routing category not applied: %+v", upd.Category)
	}
	if upd.Priority == nil || *upd.Priority != "high" {
		t.Errorf("routing priority not applied: %+v", upd.Priority)
	}
	if upd.TeamID == nil || *upd.TeamID != "team-fin" {
		t.Errorf("routing team not applied: %+v", upd.TeamID)
	}
	if pub.countExecuted() != 1 {
		t.Errorf("ai_action.executed emitted %d times, want 1", pub.countExecuted())
	}
	if store.action.Status != "executed" {
		t.Errorf("action status = %q, want executed", store.action.Status)
	}
}

func TestProcess_DuplicateDelivery_AppliesOnce(t *testing.T) {
	exec, _, tickets, pub := fixture()
	ev := reviewedEvent("org-1", "act-1", "approved")

	for i := range 3 {
		if got := exec.process(context.Background(), ev); got != outcomeAck {
			t.Fatalf("delivery %d outcome = %v, want outcomeAck", i, got)
		}
	}
	if tickets.count() != 1 {
		t.Errorf("UpdateTicket called %d times across 3 deliveries, want 1", tickets.count())
	}
	if pub.countExecuted() != 1 {
		t.Errorf("ai_action.executed emitted %d times across 3 deliveries, want 1", pub.countExecuted())
	}
}

func TestProcess_ConcurrentDuplicateDelivery_AppliesOnce(t *testing.T) {
	exec, _, tickets, pub := fixture()
	ev := reviewedEvent("org-1", "act-1", "approved")

	const n = 16
	var wg sync.WaitGroup
	wg.Add(n)
	for range n {
		go func() {
			defer wg.Done()
			exec.process(context.Background(), ev)
		}()
	}
	wg.Wait()

	if tickets.count() != 1 {
		t.Errorf("UpdateTicket called %d times under concurrency, want exactly 1", tickets.count())
	}
	if pub.countExecuted() != 1 {
		t.Errorf("ai_action.executed emitted %d times under concurrency, want exactly 1", pub.countExecuted())
	}
}

func TestProcess_RejectedDecision_NoOp(t *testing.T) {
	exec, store, tickets, pub := fixture()
	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "rejected")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if tickets.count() != 0 || pub.countExecuted() != 0 {
		t.Errorf("rejected decision applied side effects: updates=%d executed=%d", tickets.count(), pub.countExecuted())
	}
	if store.claimCalls != 0 {
		t.Errorf("rejected decision attempted a claim (%d)", store.claimCalls)
	}
}

func TestProcess_WrongKind_Skipped(t *testing.T) {
	exec, store, tickets, pub := fixture()
	store.action.Kind = "draft.reply"
	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if tickets.count() != 0 || pub.countExecuted() != 0 || store.claimCalls != 0 {
		t.Errorf("non-ticket.classification kind was executed: updates=%d executed=%d claims=%d", tickets.count(), pub.countExecuted(), store.claimCalls)
	}
}

func TestProcess_ForeignOrg_NotFoundNoApply(t *testing.T) {
	exec, store, tickets, pub := fixture()
	// Action belongs to org-1; an event for org-2 must resolve to ErrNotFound
	// and never touch it.
	if got := exec.process(context.Background(), reviewedEvent("org-2", "act-1", "approved")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if tickets.count() != 0 || pub.countExecuted() != 0 || store.claimCalls != 0 {
		t.Errorf("foreign-org event applied side effects: updates=%d executed=%d claims=%d", tickets.count(), pub.countExecuted(), store.claimCalls)
	}
	if store.action.Status != "approved" {
		t.Errorf("foreign-org event mutated the action status to %q", store.action.Status)
	}
}

func TestProcess_TransientGetError_Retries(t *testing.T) {
	exec, store, tickets, _ := fixture()
	store.getErr = errors.New("db unavailable")
	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry", got)
	}
	if tickets.count() != 0 {
		t.Errorf("transient get error still promoted (%d)", tickets.count())
	}
}

func TestProcess_PromoteFailure_RollsBackClaimAndRetries(t *testing.T) {
	exec, store, _, pub := fixture()
	exec.tickets = &fakeTickets{err: errors.New("update failed")}

	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry", got)
	}
	if pub.countExecuted() != 0 {
		t.Errorf("executed event emitted despite promote failure (%d)", pub.countExecuted())
	}
	if store.unclaimCalls != 1 {
		t.Errorf("claim was not rolled back: unclaimCalls=%d, want 1", store.unclaimCalls)
	}
	if store.action.Status != "approved" {
		t.Errorf("action status after rollback = %q, want approved (eligible for redelivery)", store.action.Status)
	}
}

func TestProcess_MissingTicket_SkipsWithoutClaiming(t *testing.T) {
	exec, store, tickets, pub := fixture()
	store.ticket = nil // no suggested ticket to promote
	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if store.claimCalls != 0 {
		t.Errorf("claimed despite no ticket to promote (%d)", store.claimCalls)
	}
	if tickets.count() != 0 || pub.countExecuted() != 0 {
		t.Errorf("applied side effects with no ticket: updates=%d executed=%d", tickets.count(), pub.countExecuted())
	}
}

func TestProcess_MalformedEvent_Acks(t *testing.T) {
	exec, _, tickets, pub := fixture()
	// Missing ai_action_id.
	ev := conversation.LifecycleEvent{OrgID: "org-1", Data: map[string]any{"decision": "approved"}}
	if got := exec.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if tickets.count() != 0 || pub.countExecuted() != 0 {
		t.Errorf("malformed event applied side effects")
	}
}

// --- draft.reply act-leg ---

func TestProcess_ApprovedDraftReply_SendsOnceAndEmitsExecuted(t *testing.T) {
	exec, store, sender, pub := draftFixture()
	sender.result = &integration.SendResult{ProviderMessageID: "slack-ts-1", Operation: "message.send"}

	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if sender.count() != 1 {
		t.Errorf("Send called %d times, want 1", sender.count())
	}
	if pub.countExecuted() != 1 {
		t.Errorf("ai_action.executed emitted %d times, want 1", pub.countExecuted())
	}
	if pub.countSubject(conversation.SubjectAIActionSendFailed) != 0 {
		t.Errorf("send_failed emitted on a successful send")
	}
	if store.action.Status != "executed" {
		t.Errorf("action status = %q, want executed", store.action.Status)
	}
}

func TestProcess_DraftReply_DuplicateDelivery_SendsOnce(t *testing.T) {
	exec, _, sender, pub := draftFixture()
	ev := reviewedEvent("org-1", "act-1", "approved")

	for i := range 3 {
		if got := exec.process(context.Background(), ev); got != outcomeAck {
			t.Fatalf("delivery %d outcome = %v, want outcomeAck", i, got)
		}
	}
	if sender.count() != 1 {
		t.Errorf("Send called %d times across 3 deliveries, want 1 (no double-send)", sender.count())
	}
	if pub.countExecuted() != 1 {
		t.Errorf("ai_action.executed emitted %d times across 3 deliveries, want 1", pub.countExecuted())
	}
}

func TestProcess_DraftReply_ConcurrentDelivery_SendsOnce(t *testing.T) {
	exec, _, sender, pub := draftFixture()
	ev := reviewedEvent("org-1", "act-1", "approved")

	const n = 16
	var wg sync.WaitGroup
	wg.Add(n)
	for range n {
		go func() {
			defer wg.Done()
			exec.process(context.Background(), ev)
		}()
	}
	wg.Wait()

	if sender.count() != 1 {
		t.Errorf("Send called %d times under concurrent redelivery, want exactly 1 (no double-send)", sender.count())
	}
	if pub.countExecuted() != 1 {
		t.Errorf("ai_action.executed emitted %d times under concurrency, want exactly 1", pub.countExecuted())
	}
}

func TestProcess_DraftReply_TerminalSendFailure_EmitsSendFailedNoUnclaim(t *testing.T) {
	exec, store, sender, pub := draftFixture()
	// Terminal: a 4xx-class failure must not be retried forever.
	sender.err = &integration.SendError{Terminal: true, Status: 400, Code: "invalid_body", Message: "bad request"}

	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck (terminal failure must not retry)", got)
	}
	if sender.count() != 1 {
		t.Errorf("Send called %d times, want 1", sender.count())
	}
	if pub.countSubject(conversation.SubjectAIActionSendFailed) != 1 {
		t.Errorf("send_failed emitted %d times, want exactly 1", pub.countSubject(conversation.SubjectAIActionSendFailed))
	}
	if pub.countExecuted() != 0 {
		t.Errorf("executed emitted on a terminal send failure")
	}
	if store.unclaimCalls != 0 {
		t.Errorf("terminal failure unclaimed (%d); must stay claimed to avoid retry-forever", store.unclaimCalls)
	}
	if store.action.Status != "executed" {
		t.Errorf("action status = %q, want executed (claim retained after terminal failure)", store.action.Status)
	}
}

func TestProcess_DraftReply_TransientSendFailure_UnclaimsAndRetries(t *testing.T) {
	exec, store, sender, pub := draftFixture()
	// Transient: a 5xx/timeout-class failure should unclaim and retry.
	sender.err = &integration.SendError{Terminal: false, Status: 502, Code: "upstream", Message: "down"}

	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry (transient failure retries)", got)
	}
	if store.unclaimCalls != 1 {
		t.Errorf("transient failure did not unclaim: unclaimCalls=%d, want 1", store.unclaimCalls)
	}
	if store.action.Status != "approved" {
		t.Errorf("action status after rollback = %q, want approved (eligible for redelivery)", store.action.Status)
	}
	if pub.countExecuted() != 0 || pub.countSubject(conversation.SubjectAIActionSendFailed) != 0 {
		t.Errorf("transient failure emitted a terminal event")
	}
}

func TestProcess_DraftReply_ForeignOrg_NeverSends(t *testing.T) {
	exec, store, sender, pub := draftFixture()
	if got := exec.process(context.Background(), reviewedEvent("org-2", "act-1", "approved")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if sender.count() != 0 || store.claimCalls != 0 {
		t.Errorf("foreign-org draft.reply sent or claimed: sends=%d claims=%d", sender.count(), store.claimCalls)
	}
	if pub.countExecuted() != 0 {
		t.Errorf("foreign-org draft.reply emitted executed")
	}
}

func TestProcess_DraftReply_MissingThreadRef_SkipsWithoutClaiming(t *testing.T) {
	exec, store, sender, _ := draftFixture()
	store.threadRef = nil // no channel ref to address the send

	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if store.claimCalls != 0 {
		t.Errorf("claimed despite no thread ref (%d)", store.claimCalls)
	}
	if sender.count() != 0 {
		t.Errorf("sent despite no thread ref (%d)", sender.count())
	}
}

func TestProcess_DraftReply_NoSenderConfigured_SkipsWithoutClaiming(t *testing.T) {
	exec, store, _, pub := draftFixture()
	exec.sender = nil // integration not configured

	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if store.claimCalls != 0 {
		t.Errorf("claimed despite no sender configured (%d)", store.claimCalls)
	}
	if pub.countExecuted() != 0 {
		t.Errorf("emitted executed despite no sender configured")
	}
}

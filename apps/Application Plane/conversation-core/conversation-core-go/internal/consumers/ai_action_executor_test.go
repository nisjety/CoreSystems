package consumers

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

	claimWins         int // claims that observed RowsAffected == 1
	claimCalls        int
	unclaimCalls      int
	outboundIntents   map[string]*conversation.OutboundIntent
	finalizedMessages int
	outboundClaimErr  error
	finalizeErr       error
	outcomeErr        error
	outcomeFailures   int
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

func (f *fakeStore) ClaimOutboundIntent(_ context.Context, input conversation.OutboundIntentClaimInput) (*conversation.OutboundIntentClaim, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.outboundClaimErr != nil {
		return nil, f.outboundClaimErr
	}
	if f.outboundIntents == nil {
		f.outboundIntents = make(map[string]*conversation.OutboundIntent)
	}
	key := input.OrgID + ":" + input.IdempotencyKey
	if existing := f.outboundIntents[key]; existing != nil {
		if existing.RequestFingerprint != input.RequestFingerprint || existing.AIActionID != input.AIActionID ||
			existing.AuthorizationKind != input.AuthorizationKind || existing.ActorUserID != input.ActorUserID ||
			existing.ApprovalID != input.ApprovalID || existing.ActionID != input.ActionID ||
			existing.Operation != input.Operation || existing.PayloadSHA256 != input.PayloadSHA256 {
			return nil, conversation.ErrConflict
		}
		if existing.Status == conversation.OutboundIntentRetryable {
			existing.Status = conversation.OutboundIntentSending
			return &conversation.OutboundIntentClaim{Intent: *existing, Claimed: true}, nil
		}
		return &conversation.OutboundIntentClaim{Intent: *existing, Claimed: false}, nil
	}
	intent := &conversation.OutboundIntent{
		ID: input.IntentID, OrgID: input.OrgID, IdempotencyKey: input.IdempotencyKey,
		ConversationID: input.ConversationID, AIActionID: input.AIActionID,
		RequestFingerprint: input.RequestFingerprint, Status: conversation.OutboundIntentSending,
		Provider: input.Provider, ConnectionID: input.ConnectionID, ProviderThreadID: input.ProviderThreadID,
		AuthorizationKind: input.AuthorizationKind, ActorUserID: input.ActorUserID,
		ApprovalID: input.ApprovalID, ActionID: input.ActionID, Operation: input.Operation, PayloadSHA256: input.PayloadSHA256,
	}
	f.outboundIntents[key] = intent
	return &conversation.OutboundIntentClaim{Intent: *intent, Claimed: true}, nil
}

func (f *fakeStore) FinalizeOutboundIntent(_ context.Context, input conversation.OutboundIntentFinalizeInput) (*conversation.Message, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.finalizeErr != nil {
		return nil, f.finalizeErr
	}
	intent := f.outboundIntents[input.OrgID+":"+input.IdempotencyKey]
	if intent == nil || intent.Status != conversation.OutboundIntentSending || intent.RequestFingerprint != input.RequestFingerprint {
		return nil, conversation.ErrConflict
	}
	intent.Status = conversation.OutboundIntentSubmitted
	intent.ProviderMessageID = input.ProviderMessageID
	intent.MessageID = "msg-ai-1"
	if f.action != nil && f.action.ID == input.AIActionID && f.action.Status == "approved" {
		f.action.Status = "executed"
	}
	f.finalizedMessages++
	return &conversation.Message{
		ID: intent.MessageID, OrgID: input.OrgID, ConversationID: input.Message.ConversationID,
		Direction: conversation.DirectionOutbound, BodyText: input.Message.BodyText,
		Provider: intent.Provider, ProviderMessageID: input.ProviderMessageID,
	}, nil
}

func (f *fakeStore) MarkOutboundIntentOutcome(_ context.Context, input conversation.OutboundIntentOutcomeInput) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.outcomeFailures > 0 {
		f.outcomeFailures--
		return errors.New("transient outcome persistence failure")
	}
	if f.outcomeErr != nil {
		return f.outcomeErr
	}
	intent := f.outboundIntents[input.OrgID+":"+input.IdempotencyKey]
	if intent == nil {
		return conversation.ErrNotFound
	}
	intent.Status = input.Status
	intent.ErrorCode = input.ErrorCode
	if input.Status != conversation.OutboundIntentRetryable && f.action != nil && f.action.ID == input.AIActionID && f.action.Status == "approved" {
		f.action.Status = input.Status
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
	events   []conversation.LifecycleEvent
}

func (f *fakePublisher) Publish(_ context.Context, subject string, payload any) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.subjects = append(f.subjects, subject)
	if event, ok := payload.(conversation.LifecycleEvent); ok {
		f.events = append(f.events, event)
	}
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

func (f *fakePublisher) lastEvent() conversation.LifecycleEvent {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.events[len(f.events)-1]
}

// fakeSender records send attempts behind a mutex so concurrent deliveries race
// exactly as a real send would, letting the no-double-send test assert exactly
// one Send call survives the atomic claim.
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
			ReviewedBy:     "reviewer-1",
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
	if sender.lastReq.AuthorizationKind != "human_approved_ai_action" || sender.lastReq.ApprovalID != "act-1" || sender.lastReq.ActionID != "act-1" || sender.lastReq.AuthorizationID == "" || sender.lastReq.ActorUserID != "reviewer-1" || sender.lastReq.PayloadSHA256 == "" {
		t.Fatalf("AI authorization binding = %#v", sender.lastReq)
	}
	if sender.lastReq.IdempotencyKey != "conversation-ai:act-1" {
		t.Fatalf("idempotency = %q, want approved action receipt contract", sender.lastReq.IdempotencyKey)
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

func TestProcess_DraftReplyRequiresDurableReviewerAndIgnoresEventActor(t *testing.T) {
	exec, store, sender, _ := draftFixture()
	store.action.ReviewedBy = ""
	event := reviewedEvent("org-1", "act-1", "approved")
	event.ActorUserID = "forged-event-actor"
	if got := exec.process(t.Context(), event); got != outcomeAck || sender.count() != 0 || len(store.outboundIntents) != 0 {
		t.Fatalf("reviewer-less action outcome=%v sends=%d intents=%d", got, sender.count(), len(store.outboundIntents))
	}

	store.action.ReviewedBy = "durable-reviewer"
	if got := exec.process(t.Context(), event); got != outcomeAck || sender.count() != 1 {
		t.Fatalf("reviewed action outcome=%v sends=%d", got, sender.count())
	}
	if sender.lastReq.ActorUserID != "durable-reviewer" {
		t.Fatalf("actor = %q, want durable reviewer", sender.lastReq.ActorUserID)
	}
}

func TestProcess_DraftReplyPreProviderFailureNaksAndSafelyReclaims(t *testing.T) {
	exec, store, sender, _ := draftFixture()
	sender.err = &integration.SendError{SafeToRetry: true, Code: "auth_token_unavailable", Message: "pre-provider"}
	event := reviewedEvent("org-1", "act-1", "approved")
	if got := exec.process(t.Context(), event); got != outcomeRetry {
		t.Fatalf("first outcome = %v, want retry", got)
	}
	intent := store.outboundIntents["org-1:conversation-ai:act-1"]
	if intent == nil || intent.Status != conversation.OutboundIntentRetryable || store.action.Status != "approved" {
		t.Fatalf("intent/action = %#v/%q", intent, store.action.Status)
	}
	sender.mu.Lock()
	sender.err = nil
	sender.mu.Unlock()
	if got := exec.process(t.Context(), event); got != outcomeAck {
		t.Fatalf("second outcome = %v, want ack", got)
	}
	if sender.count() != 2 || store.action.Status != "executed" {
		t.Fatalf("sends/action = %d/%q", sender.count(), store.action.Status)
	}
}

func TestProcess_DraftReplyRetriesRetryableStatePersistenceBeforeNak(t *testing.T) {
	exec, store, sender, _ := draftFixture()
	store.outcomeFailures = 1
	sender.err = &integration.SendError{SafeToRetry: true, Code: "action_pre_provider_retryable", Message: "pre-provider"}
	event := reviewedEvent("org-1", "act-1", "approved")
	if got := exec.process(t.Context(), event); got != outcomeRetry {
		t.Fatalf("first outcome = %v, want retry", got)
	}
	intent := store.outboundIntents["org-1:conversation-ai:act-1"]
	if intent == nil || intent.Status != conversation.OutboundIntentRetryable || store.outcomeFailures != 0 {
		t.Fatalf("intent/failures = %#v/%d", intent, store.outcomeFailures)
	}
	sender.mu.Lock()
	sender.err = nil
	sender.mu.Unlock()
	if got := exec.process(t.Context(), event); got != outcomeAck || sender.count() != 2 || store.action.Status != "executed" {
		t.Fatalf("redelivery outcome/sends/action = %v/%d/%q", got, sender.count(), store.action.Status)
	}
}

func TestProcess_DraftReplyFailedRetryablePersistenceIsVisibleAndNeverResent(t *testing.T) {
	exec, store, sender, publisher := draftFixture()
	store.outcomeFailures = 100
	sender.err = &integration.SendError{SafeToRetry: true, Code: "action_pre_provider_retryable", Message: "pre-provider"}
	event := reviewedEvent("org-1", "act-1", "approved")
	if got := exec.process(t.Context(), event); got != outcomeAck {
		t.Fatalf("outcome = %v, want reconciliation ACK", got)
	}
	if publisher.countSubject(conversation.SubjectAIActionSendUnknown) != 1 {
		t.Fatalf("send_unknown events = %d", publisher.countSubject(conversation.SubjectAIActionSendUnknown))
	}
	if got := exec.process(t.Context(), event); got != outcomeAck || sender.count() != 1 {
		t.Fatalf("redelivery outcome/sends = %v/%d, want no provider retry", got, sender.count())
	}
}

func TestProcess_DraftReplyRealHTTPPreProviderSentinelRedeliversWithFreshJTI(t *testing.T) {
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
	signer, err := attestation.NewSigner(attestation.Config{
		PrivateKey: ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef")),
		KeyID:      "test-key", Issuer: attestation.IssuerConversationCore,
		Audience: attestation.AudienceIntegrationCore, Presenter: attestation.PresenterConversationCore,
		Now:    func() time.Time { return time.Date(2026, time.July, 13, 18, 0, 0, 0, time.UTC) },
		Random: bytes.NewReader(append(bytes.Repeat([]byte{3}, 16), bytes.Repeat([]byte{4}, 16)...)),
	})
	if err != nil {
		t.Fatal(err)
	}
	client := integration.NewClient(actionServer.URL, "internal",
		integration.WithServicePrincipal(authServer.URL, "conversation-core", "credential"),
		integration.WithWriteAttestor(signer),
	)
	executor, store, _, _ := draftFixture()
	executor.sender = client
	event := reviewedEvent("org-1", "act-1", "approved")
	if got := executor.process(t.Context(), event); got != outcomeRetry {
		t.Fatalf("first outcome = %v", got)
	}
	intent := store.outboundIntents["org-1:conversation-ai:act-1"]
	if intent == nil || intent.Status != conversation.OutboundIntentRetryable {
		t.Fatalf("first intent = %#v", intent)
	}
	if got := executor.process(t.Context(), event); got != outcomeAck || store.action.Status != "executed" {
		t.Fatalf("redelivery outcome/action = %v/%q", got, store.action.Status)
	}
	if len(attestations) != 2 || attestations[0] == attestations[1] {
		t.Fatalf("attestations = %#v", attestations)
	}
	first := decodeExecutorAttestationClaims(t, attestations[0])
	second := decodeExecutorAttestationClaims(t, attestations[1])
	if first.AuthorizationID != second.AuthorizationID || first.PayloadSHA256 != second.PayloadSHA256 || first.JWTID == second.JWTID {
		t.Fatalf("claim bindings = %#v / %#v", first, second)
	}
}

func decodeExecutorAttestationClaims(t *testing.T, compact string) attestation.Claims {
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

func TestProcess_DraftReply_TerminalSendFailure_PersistsFailedNotExecuted(t *testing.T) {
	exec, store, sender, pub := draftFixture()
	// Terminal: a 4xx-class failure must not be retried forever.
	sender.err = &integration.SendError{Terminal: true, Status: 400, Code: "invalid_body", Message: "sensitive provider response"}

	if got := exec.process(context.Background(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck (terminal failure must not retry)", got)
	}
	if sender.count() != 1 {
		t.Errorf("Send called %d times, want 1", sender.count())
	}
	if pub.countSubject(conversation.SubjectAIActionSendFailed) != 1 {
		t.Errorf("send_failed emitted %d times, want exactly 1", pub.countSubject(conversation.SubjectAIActionSendFailed))
	}
	failureData := pub.lastEvent().Data
	if failureData["error_code"] != "invalid_body" {
		t.Fatalf("send_failed error_code = %v, want invalid_body", failureData["error_code"])
	}
	if _, leaked := failureData["error"]; leaked {
		t.Fatalf("send_failed event leaked raw error: %#v", failureData)
	}
	if pub.countExecuted() != 0 {
		t.Errorf("executed emitted on a terminal send failure")
	}
	if store.unclaimCalls != 0 {
		t.Errorf("terminal failure unclaimed (%d); must stay claimed to avoid retry-forever", store.unclaimCalls)
	}
	if store.action.Status != "failed" {
		t.Errorf("action status = %q, want failed", store.action.Status)
	}
}

func TestProcess_DraftReply_TransientSendFailure_PersistsUnknownAndDoesNotBlindRetry(t *testing.T) {
	exec, store, sender, pub := draftFixture()
	// A 5xx/timeout is ambiguous: the provider may have accepted the request.
	sender.err = &integration.SendError{Terminal: false, Status: 502, Code: "upstream", Message: "down"}
	ev := reviewedEvent("org-1", "act-1", "approved")

	for attempt := 1; attempt <= 2; attempt++ {
		if got := exec.process(context.Background(), ev); got != outcomeAck {
			t.Fatalf("attempt %d outcome = %v, want outcomeAck (unknown must not redeliver)", attempt, got)
		}
	}
	if sender.count() != 1 {
		t.Fatalf("Send called %d times, want 1 while outcome is unknown", sender.count())
	}
	if store.unclaimCalls != 0 {
		t.Errorf("unknown outcome was unclaimed %d times", store.unclaimCalls)
	}
	if store.action.Status != "unknown" {
		t.Errorf("action status = %q, want unknown", store.action.Status)
	}
	if pub.countExecuted() != 0 || pub.countSubject(conversation.SubjectAIActionSendUnknown) != 1 {
		t.Errorf("unknown outcome events: executed=%d unknown=%d", pub.countExecuted(), pub.countSubject(conversation.SubjectAIActionSendUnknown))
	}
}

func TestProcess_DraftReply_FinalizeFailureBecomesUnknownWithoutResend(t *testing.T) {
	exec, store, sender, pub := draftFixture()
	store.finalizeErr = errors.New("database unavailable after provider acceptance")
	ev := reviewedEvent("org-1", "act-1", "approved")

	if got := exec.process(t.Context(), ev); got != outcomeAck {
		t.Fatalf("first outcome = %v", got)
	}
	if got := exec.process(t.Context(), ev); got != outcomeAck {
		t.Fatalf("replay outcome = %v", got)
	}
	if sender.count() != 1 {
		t.Fatalf("Send called %d times, want 1", sender.count())
	}
	if store.action.Status != "unknown" || pub.countSubject(conversation.SubjectAIActionSendUnknown) != 1 {
		t.Fatalf("status/events = %q/%d", store.action.Status, pub.countSubject(conversation.SubjectAIActionSendUnknown))
	}
}

func TestProcess_DraftReply_ClaimFailuresDoNotReachProvider(t *testing.T) {
	t.Run("authority unavailable retries before provider", func(t *testing.T) {
		exec, store, sender, _ := draftFixture()
		store.outboundClaimErr = errors.New("database unavailable")
		if got := exec.process(t.Context(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeRetry {
			t.Fatalf("outcome = %v, want retry", got)
		}
		if sender.count() != 0 {
			t.Fatalf("Send called %d times", sender.count())
		}
	})

	t.Run("conflicting intent acks without provider", func(t *testing.T) {
		exec, store, sender, _ := draftFixture()
		store.outboundIntents = map[string]*conversation.OutboundIntent{
			"org-1:conversation-ai:act-1": {
				OrgID: "org-1", IdempotencyKey: "conversation-ai:act-1", AIActionID: "act-1",
				RequestFingerprint: "different", Status: conversation.OutboundIntentSubmitted,
			},
		}
		if got := exec.process(t.Context(), reviewedEvent("org-1", "act-1", "approved")); got != outcomeAck {
			t.Fatalf("outcome = %v, want ack", got)
		}
		if sender.count() != 0 {
			t.Fatalf("Send called %d times", sender.count())
		}
	})
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

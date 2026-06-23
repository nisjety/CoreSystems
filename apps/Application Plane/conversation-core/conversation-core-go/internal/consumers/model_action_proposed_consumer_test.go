package consumers

import (
	"context"
	"fmt"
	"sync"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

// fakeProposer records CreateAIAction calls and lets a test inject an error so
// the consumer's ack-vs-retry policy can be asserted.
type fakeProposer struct {
	mu      sync.Mutex
	inputs  []conversation.CreateAIActionInput
	err     error
	invalid bool // when set, err is an ErrInvalidInput-wrapped error
}

func (f *fakeProposer) CreateAIAction(_ context.Context, input conversation.CreateAIActionInput) (*conversation.AIAction, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.err != nil {
		return nil, f.err
	}
	f.inputs = append(f.inputs, input)
	return &conversation.AIAction{
		ID:             "act-new",
		OrgID:          input.OrgID,
		ConversationID: input.ConversationID,
		Kind:           input.Kind,
		Status:         "suggested",
		Payload:        input.Payload,
	}, nil
}

func (f *fakeProposer) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.inputs)
}

func proposedEvent(orgID, conversationID, kind string) conversation.LifecycleEvent {
	return conversation.LifecycleEvent{
		Type:           "model.action.proposed",
		OrgID:          orgID,
		ConversationID: conversationID,
		Data: map[string]any{
			"kind": kind,
			"payload": map[string]any{
				"body_text": "draft reply text",
			},
		},
	}
}

func TestModelProposed_ValidEvent_CreatesOneAction(t *testing.T) {
	proposer := &fakeProposer{}
	c := &ModelActionProposedConsumer{service: proposer}

	if got := c.process(context.Background(), proposedEvent("org-1", "conv-1", "draft.reply")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if proposer.count() != 1 {
		t.Fatalf("CreateAIAction called %d times, want 1", proposer.count())
	}
	in := proposer.inputs[0]
	if in.OrgID != "org-1" || in.ConversationID != "conv-1" || in.Kind != "draft.reply" {
		t.Errorf("created action input mismatch: %+v", in)
	}
	if in.CreatedBy != "model-plane" {
		t.Errorf("created_by = %q, want model-plane", in.CreatedBy)
	}
	if in.Payload == nil || in.Payload["body_text"] != "draft reply text" {
		t.Errorf("payload not forwarded: %+v", in.Payload)
	}
}

func TestModelProposed_ConversationIDFromData(t *testing.T) {
	proposer := &fakeProposer{}
	c := &ModelActionProposedConsumer{service: proposer}
	ev := conversation.LifecycleEvent{
		OrgID: "org-1",
		Data:  map[string]any{"conversation_id": "conv-9", "kind": "draft.reply"},
	}
	if got := c.process(context.Background(), ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if proposer.count() != 1 || proposer.inputs[0].ConversationID != "conv-9" {
		t.Errorf("conversation_id not read from data: %+v", proposer.inputs)
	}
}

func TestModelProposed_MalformedEvent_AcksWithoutCreating(t *testing.T) {
	cases := []conversation.LifecycleEvent{
		{Data: map[string]any{"kind": "draft.reply"}},                       // missing org + conversation
		{OrgID: "org-1", Data: map[string]any{"kind": "draft.reply"}},       // missing conversation
		{OrgID: "org-1", ConversationID: "conv-1", Data: map[string]any{}},  // missing kind
	}
	for i, ev := range cases {
		proposer := &fakeProposer{}
		c := &ModelActionProposedConsumer{service: proposer}
		if got := c.process(context.Background(), ev); got != outcomeAck {
			t.Fatalf("case %d outcome = %v, want outcomeAck", i, got)
		}
		if proposer.count() != 0 {
			t.Errorf("case %d created an action from a malformed event", i)
		}
	}
}

func TestModelProposed_InvalidKind_AcksWithoutRetry(t *testing.T) {
	// A disallowed kind is terminal — the consumer must ack (not loop forever).
	proposer := &fakeProposer{err: fmt.Errorf("%w: unsupported action kind", conversation.ErrInvalidInput)}
	c := &ModelActionProposedConsumer{service: proposer}
	if got := c.process(context.Background(), proposedEvent("org-1", "conv-1", "ticket.delete")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck for an invalid kind", got)
	}
}

func TestModelProposed_TransientStoreError_Retries(t *testing.T) {
	proposer := &fakeProposer{err: fmt.Errorf("db unavailable")}
	c := &ModelActionProposedConsumer{service: proposer}
	if got := c.process(context.Background(), proposedEvent("org-1", "conv-1", "draft.reply")); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry for a transient store error", got)
	}
}

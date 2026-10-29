package conversation

import (
	"context"
	"errors"
	"testing"
)

type proposalPolicyStub struct {
	err   error
	calls int
}

func (s *proposalPolicyStub) AllowAIProposal(_ context.Context, _ string) error {
	s.calls++
	return s.err
}

func TestCreateAIActionEnforcesPolicyBeforeConversationOrPersistence(t *testing.T) {
	repository := newFakeRepository()
	policy := &proposalPolicyStub{err: ErrZDRAIProposalForbidden}
	service := NewService(repository, nil, WithAIProposalPolicy(policy))

	_, err := service.CreateAIAction(t.Context(), CreateAIActionInput{
		OrgID:          "org_1",
		ConversationID: "conv_1",
		Kind:           "draft.reply",
		Payload:        map[string]any{"body_text": "Hello"},
		CreatedBy:      "agent_1",
	})
	if !errors.Is(err, ErrZDRAIProposalForbidden) {
		t.Fatalf("error = %v, want ZDR policy denial", err)
	}
	if policy.calls != 1 {
		t.Fatalf("policy calls = %d, want 1", policy.calls)
	}
	if len(repository.aiActions) != 0 {
		t.Fatalf("persisted AI actions = %d, want 0", len(repository.aiActions))
	}
}

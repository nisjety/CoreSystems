package http

import (
	"context"
	stdhttp "net/http"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

// recordingFeedbackRepository is a conversation.Repository test double that
// records the inbound event SubmitFeedback stores and the tag it applies,
// without needing a real Postgres-backed conversation-core.
type recordingFeedbackRepository struct {
	conversation.Repository
	storedEvent conversation.InboundEvent
	storeCalls  int
	taggedID    string
	taggedTag   string
	tagCalls    int
}

func (r *recordingFeedbackRepository) StoreInboundEvent(_ context.Context, event conversation.InboundEvent) (*conversation.StoredEventResult, error) {
	r.storeCalls++
	r.storedEvent = event
	detail := &conversation.ConversationDetail{
		ConversationSummary: conversation.ConversationSummary{
			ID: "feedback-conversation-1", OrgID: event.OrgID, Title: event.Subject, Provider: event.Provider,
		},
	}
	return &conversation.StoredEventResult{
		Detail:  detail,
		Message: &conversation.Message{ID: "feedback-message-1", OrgID: event.OrgID, BodyText: event.BodyText},
		Created: true,
	}, nil
}

func (r *recordingFeedbackRepository) AddTag(_ context.Context, _ string, conversationID, tag string) (*conversation.ConversationDetail, error) {
	r.tagCalls++
	r.taggedID = conversationID
	r.taggedTag = tag
	return &conversation.ConversationDetail{
		ConversationSummary: conversation.ConversationSummary{ID: conversationID, Tags: []string{tag}},
	}, nil
}

func TestFeedbackRouteCreatesTaggedConversation(t *testing.T) {
	repository := &recordingFeedbackRepository{}
	service := conversation.NewService(repository, nil)
	router := newRouter(NewHandler(nil, service), testVerifier(t))
	body := []byte(`{"body_text":"The knowledge tab spinner never resolves.","from_name":"Ada","idempotency_key":"demo-feedback-http-0001"}`)

	response := performRequest(router, signedAgentRequest(t, stdhttp.MethodPost, "/api/v1/feedback", body))

	if response.Code != stdhttp.StatusCreated {
		t.Fatalf("status = %d, want 201; body=%s", response.Code, response.Body.String())
	}
	if repository.storeCalls != 1 {
		t.Fatalf("StoreInboundEvent calls = %d, want 1", repository.storeCalls)
	}
	if repository.storedEvent.Provider != conversation.FeedbackProvider {
		t.Fatalf("provider = %q, want %q", repository.storedEvent.Provider, conversation.FeedbackProvider)
	}
	if repository.storedEvent.Direction != conversation.DirectionInbound {
		t.Fatalf("direction = %q, want inbound", repository.storedEvent.Direction)
	}
	if repository.tagCalls != 1 || repository.taggedID != "feedback-conversation-1" || repository.taggedTag != conversation.FeedbackTag {
		t.Fatalf("tag write = (%d calls, id=%q, tag=%q), want (1, feedback-conversation-1, %q)",
			repository.tagCalls, repository.taggedID, repository.taggedTag, conversation.FeedbackTag)
	}
	if !strings.Contains(response.Body.String(), `"pilot-feedback"`) {
		t.Fatalf("body = %s, want the pilot-feedback tag in the response", response.Body.String())
	}
}

func TestFeedbackRouteRejectsMissingBodyText(t *testing.T) {
	repository := &recordingFeedbackRepository{}
	service := conversation.NewService(repository, nil)
	router := newRouter(NewHandler(nil, service), testVerifier(t))
	body := []byte(`{"idempotency_key":"demo-feedback-http-0002"}`)

	response := performRequest(router, signedAgentRequest(t, stdhttp.MethodPost, "/api/v1/feedback", body))

	if response.Code != stdhttp.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want 422; body=%s", response.Code, response.Body.String())
	}
	if repository.storeCalls != 0 {
		t.Fatalf("StoreInboundEvent calls = %d, want 0 for a rejected submission", repository.storeCalls)
	}
}

func TestFeedbackRouteRejectsMissingIdempotencyKey(t *testing.T) {
	repository := &recordingFeedbackRepository{}
	service := conversation.NewService(repository, nil)
	router := newRouter(NewHandler(nil, service), testVerifier(t))
	body := []byte(`{"body_text":"Missing an idempotency key."}`)

	response := performRequest(router, signedAgentRequest(t, stdhttp.MethodPost, "/api/v1/feedback", body))

	if response.Code != stdhttp.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want 422; body=%s", response.Code, response.Body.String())
	}
	if repository.storeCalls != 0 {
		t.Fatalf("StoreInboundEvent calls = %d, want 0 for a rejected submission", repository.storeCalls)
	}
}

package http

import (
	"bytes"
	"context"
	stdhttp "net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

type recordingMessageRepository struct {
	conversation.Repository
	addMessageInputs []conversation.AddMessageInput
}

func (r *recordingMessageRepository) AddMessage(_ context.Context, input conversation.AddMessageInput) (*conversation.Message, error) {
	r.addMessageInputs = append(r.addMessageInputs, input)
	return &conversation.Message{
		ID:             "message-1",
		OrgID:          input.OrgID,
		ConversationID: input.ConversationID,
		Direction:      input.Direction,
		BodyText:       input.BodyText,
		Internal:       input.Internal,
		OccurredAt:     input.OccurredAt,
		CreatedAt:      time.Now().UTC(),
	}, nil
}

func signedAgentRequest(t *testing.T, method, path string, body []byte) *stdhttp.Request {
	t.Helper()
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	request.Header.Set("Content-Type", "application/json")
	signConversationRequest(t, request, body, "verevon-gateway", testGatewaySecret, "user-1", "org-1", "member")
	return request
}

func TestMessagesRouteRejectsCallerSelectedInternalBeforePersistence(t *testing.T) {
	repository := &recordingMessageRepository{}
	service := conversation.NewService(repository, nil)
	router := newRouter(NewHandler(nil, service), testVerifier(t))
	body := []byte(`{"body_text":"customer reply","internal":true}`)

	response := performRequest(router, signedAgentRequest(
		t,
		stdhttp.MethodPost,
		"/api/v1/conversations/conversation-1/messages",
		body,
	))

	if response.Code != stdhttp.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want 422; body=%s", response.Code, response.Body.String())
	}
	if !strings.Contains(response.Body.String(), `"code":"validation_error"`) ||
		!strings.Contains(response.Body.String(), "/notes") {
		t.Fatalf("body = %s, want honest notes-route validation guidance", response.Body.String())
	}
	if len(repository.addMessageInputs) != 0 {
		t.Fatalf("persisted messages = %d, want 0", len(repository.addMessageInputs))
	}
}

func TestMessagesRouteRejectsNonBooleanInternalBeforePersistence(t *testing.T) {
	repository := &recordingMessageRepository{}
	service := conversation.NewService(repository, nil)
	router := newRouter(NewHandler(nil, service), testVerifier(t))
	body := []byte(`{"body_text":"customer reply","internal":"true"}`)

	response := performRequest(router, signedAgentRequest(
		t,
		stdhttp.MethodPost,
		"/api/v1/conversations/conversation-1/messages",
		body,
	))

	if response.Code != stdhttp.StatusBadRequest {
		t.Fatalf("status = %d, want 400; body=%s", response.Code, response.Body.String())
	}
	if !strings.Contains(response.Body.String(), `"code":"invalid_json"`) {
		t.Fatalf("body = %s, want invalid_json", response.Body.String())
	}
	if len(repository.addMessageInputs) != 0 {
		t.Fatalf("persisted messages = %d, want 0", len(repository.addMessageInputs))
	}
}

func TestMessagesRouteAlwaysUsesExternalDeliveryPath(t *testing.T) {
	tests := []struct {
		name string
		body []byte
	}{
		{
			name: "internal omitted",
			body: []byte(`{"body_text":"customer reply","idempotency_key":"inbox-reply-omitted-0001"}`),
		},
		{
			name: "internal false",
			body: []byte(`{"body_text":"customer reply","internal":false,"idempotency_key":"inbox-reply-false-0001"}`),
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			repository := &recordingMessageRepository{}
			service := conversation.NewService(repository, nil)
			router := newRouter(NewHandler(nil, service), testVerifier(t))

			response := performRequest(router, signedAgentRequest(
				t,
				stdhttp.MethodPost,
				"/api/v1/conversations/conversation-1/messages",
				test.body,
			))

			if response.Code != stdhttp.StatusServiceUnavailable {
				t.Fatalf("status = %d, want 503; body=%s", response.Code, response.Body.String())
			}
			if !strings.Contains(response.Body.String(), `"code":"delivery_unavailable"`) {
				t.Fatalf("body = %s, want delivery_unavailable external-send error", response.Body.String())
			}
			if len(repository.addMessageInputs) != 0 {
				t.Fatalf("persisted messages = %d, want 0", len(repository.addMessageInputs))
			}
		})
	}
}

func TestNotesRouteRemainsTheOnlyStoreOnlyMessagePath(t *testing.T) {
	repository := &recordingMessageRepository{}
	service := conversation.NewService(repository, nil)
	router := newRouter(NewHandler(nil, service), testVerifier(t))
	body := []byte(`{"body_text":"internal note","internal":false}`)

	response := performRequest(router, signedAgentRequest(
		t,
		stdhttp.MethodPost,
		"/api/v1/conversations/conversation-1/notes",
		body,
	))

	if response.Code != stdhttp.StatusCreated {
		t.Fatalf("status = %d, want 201; body=%s", response.Code, response.Body.String())
	}
	if len(repository.addMessageInputs) != 1 || !repository.addMessageInputs[0].Internal {
		t.Fatalf("stored inputs = %#v, want one forced-internal note", repository.addMessageInputs)
	}
}

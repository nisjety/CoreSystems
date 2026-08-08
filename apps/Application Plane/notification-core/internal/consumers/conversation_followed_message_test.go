package consumers

import (
	"context"
	"errors"
	"reflect"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

type conversationFollowAccepter struct {
	mu       sync.Mutex
	requests []notification.Request
	errors   map[string]error
}

func (f *conversationFollowAccepter) Accept(_ context.Context, request notification.Request) (*notification.AcceptedRequest, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.requests = append(f.requests, request)
	if err := f.errors[request.Recipient.ID]; err != nil {
		return nil, err
	}
	return &notification.AcceptedRequest{}, nil
}

func followedMessageEvent() conversationLifecycleEvent {
	return conversationLifecycleEvent{
		ID:             "event-1",
		Type:           "message.received",
		OrgID:          "org-1",
		ConversationID: "conversation-1",
		MessageID:      "message-1",
		OccurredAt:     time.Now().UTC(),
		Data: map[string]any{
			"follower_user_ids": []any{"agent-b", "agent-a", "agent-b", "not a valid id"},
		},
	}
}

func TestConversationFollowedMessageCreatesGenericNotificationPerFollower(t *testing.T) {
	accepter := &conversationFollowAccepter{}
	subscriber := &ConversationFollowedMessageSubscriber{service: accepter}

	if got := subscriber.process(context.Background(), followedMessageEvent()); got != conversationFollowAck {
		t.Fatalf("outcome = %v, want conversationFollowAck", got)
	}

	if got, want := len(accepter.requests), 2; got != want {
		t.Fatalf("Accept calls = %d, want %d", got, want)
	}
	for index, userID := range []string{"agent-a", "agent-b"} {
		request := accepter.requests[index]
		if request.OrganizationID != "org-1" || request.Recipient.ID != userID {
			t.Fatalf("request %d = %#v, want org-1/%s", index, request, userID)
		}
		if request.Type != NotificationTypeConversationFollowedMessage || request.Source != conversationFollowSource {
			t.Fatalf("request %d type/source = %q/%q", index, request.Type, request.Source)
		}
		wantKey := "conversation-followed-message:org-1:conversation-1:message-1:" + userID
		if request.IdempotencyKey != wantKey {
			t.Errorf("request %d idempotency_key = %q, want %q", index, request.IdempotencyKey, wantKey)
		}
		wantPayload := map[string]any{
			"title":           "New activity in a followed conversation",
			"body":            "A customer sent a new message in a conversation you follow.",
			"cta_label":       "Open Inbox",
			"cta_href":        "/inbox",
			"conversation_id": "conversation-1",
			"message_id":      "message-1",
		}
		if !reflect.DeepEqual(request.Payload, wantPayload) {
			t.Errorf("request %d payload = %#v, want generic content-free payload %#v", index, request.Payload, wantPayload)
		}
	}
}

func TestConversationFollowedMessageSkipsUnauthorizedFollowerWithoutRetryingOthers(t *testing.T) {
	accepter := &conversationFollowAccepter{errors: map[string]error{"agent-b": notification.ErrRecipientNotAuthorized}}
	subscriber := &ConversationFollowedMessageSubscriber{service: accepter}

	if got := subscriber.process(context.Background(), followedMessageEvent()); got != conversationFollowAck {
		t.Fatalf("outcome = %v, want conversationFollowAck", got)
	}
	if got, want := len(accepter.requests), 2; got != want {
		t.Fatalf("Accept calls = %d, want %d", got, want)
	}
}

func TestConversationFollowedMessageRetriesTransientNotificationFailure(t *testing.T) {
	accepter := &conversationFollowAccepter{errors: map[string]error{"agent-a": errors.New("database unavailable")}}
	subscriber := &ConversationFollowedMessageSubscriber{service: accepter}

	if got := subscriber.process(context.Background(), followedMessageEvent()); got != conversationFollowRetry {
		t.Fatalf("outcome = %v, want conversationFollowRetry", got)
	}
}

func TestConversationFollowedMessageTerminatesInvalidEvent(t *testing.T) {
	accepter := &conversationFollowAccepter{}
	subscriber := &ConversationFollowedMessageSubscriber{service: accepter}
	event := followedMessageEvent()
	event.MessageID = ""

	if got := subscriber.process(context.Background(), event); got != conversationFollowTerminate {
		t.Fatalf("outcome = %v, want conversationFollowTerminate", got)
	}
	if got := len(accepter.requests); got != 0 {
		t.Errorf("Accept calls = %d, want 0", got)
	}
}

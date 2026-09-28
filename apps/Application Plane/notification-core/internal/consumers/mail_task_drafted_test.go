package consumers

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

type mailTaskDraftedAccepter struct {
	mu       sync.Mutex
	requests []notification.Request
	err      error
}

func (f *mailTaskDraftedAccepter) Accept(_ context.Context, request notification.Request) (*notification.AcceptedRequest, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.requests = append(f.requests, request)
	if f.err != nil {
		return nil, f.err
	}
	return &notification.AcceptedRequest{}, nil
}

func draftedEvent() mailTaskDraftedEvent {
	return mailTaskDraftedEvent{
		ID:         "mail_task_drafted:mail_task/deadbeef",
		Type:       "mail_task_drafted",
		OrgID:      "org-1",
		UserID:     "user-1",
		ThreadID:   "thread-1",
		OccurredAt: time.Now().UTC(),
	}
}

func TestMailTaskDraftedCreatesExactlyOneNotificationForTheEventsOwnUser(t *testing.T) {
	accepter := &mailTaskDraftedAccepter{}
	subscriber := &MailTaskDraftedSubscriber{service: accepter}

	if got := subscriber.process(context.Background(), draftedEvent()); got != mailTaskDraftedAck {
		t.Fatalf("outcome = %v, want mailTaskDraftedAck", got)
	}
	if got, want := len(accepter.requests), 1; got != want {
		t.Fatalf("Accept calls = %d, want %d (single recipient, not a follower fan-out)", got, want)
	}
	request := accepter.requests[0]
	if request.OrganizationID != "org-1" || request.Recipient.ID != "user-1" || request.Recipient.Kind != notification.RecipientKindUser {
		t.Fatalf("request = %#v, want org-1/user-1", request)
	}
	if request.Type != NotificationTypeMailTaskDrafted || request.Source != mailTaskDraftedSource {
		t.Fatalf("request type/source = %q/%q", request.Type, request.Source)
	}
	wantKey := "mail-task-drafted:mail_task_drafted:mail_task/deadbeef"
	if request.IdempotencyKey != wantKey {
		t.Errorf("idempotency_key = %q, want %q", request.IdempotencyKey, wantKey)
	}
	wantPayload := map[string]any{
		"title":     "Email task drafted",
		"body":      "An email you forwarded to your task address produced a drafted answer.",
		"cta_label": "Open thread",
		"cta_href":  "/chat/thread-1",
		"thread_id": "thread-1",
	}
	if !reflect.DeepEqual(request.Payload, wantPayload) {
		t.Errorf("payload = %#v, want content-free payload %#v (no subject/body)", request.Payload, wantPayload)
	}
}

func TestMailTaskDraftedRespectsTheRecipientsNotificationPreference(t *testing.T) {
	// notification.Service.Accept is what actually checks
	// PreferenceGate.IsNotificationEnabled for NotificationTypeMailTaskDrafted
	// before creating delivery; this subscriber's own contract is that it
	// never bypasses that check by calling anything else. Simulating the
	// service's own "opted out" refusal (ErrRecipientNotAuthorized, the same
	// sentinel the real Accept returns for an org-membership/authorization
	// failure and that the delivery worker treats identically to a
	// preference opt-out) must be a silent ack, not a retry.
	accepter := &mailTaskDraftedAccepter{err: notification.ErrRecipientNotAuthorized}
	subscriber := &MailTaskDraftedSubscriber{service: accepter}

	if got := subscriber.process(context.Background(), draftedEvent()); got != mailTaskDraftedAck {
		t.Fatalf("outcome = %v, want mailTaskDraftedAck for an opted-out/unauthorized recipient", got)
	}
	if got := len(accepter.requests); got != 1 {
		t.Fatalf("Accept calls = %d, want exactly 1 (still asked once, just not retried)", got)
	}
}

func TestMailTaskDraftedRetriesTransientNotificationFailure(t *testing.T) {
	accepter := &mailTaskDraftedAccepter{err: errors.New("database unavailable")}
	subscriber := &MailTaskDraftedSubscriber{service: accepter}

	if got := subscriber.process(context.Background(), draftedEvent()); got != mailTaskDraftedRetry {
		t.Fatalf("outcome = %v, want mailTaskDraftedRetry", got)
	}
}

func TestMailTaskDraftedTerminatesAnEventMissingRequiredFields(t *testing.T) {
	accepter := &mailTaskDraftedAccepter{}
	for name, mutate := range map[string]func(*mailTaskDraftedEvent){
		"missing org_id":    func(e *mailTaskDraftedEvent) { e.OrgID = "" },
		"missing user_id":   func(e *mailTaskDraftedEvent) { e.UserID = "" },
		"missing thread_id": func(e *mailTaskDraftedEvent) { e.ThreadID = "" },
		"missing event id":  func(e *mailTaskDraftedEvent) { e.ID = "" },
		"wrong type":        func(e *mailTaskDraftedEvent) { e.Type = "something.else" },
	} {
		t.Run(name, func(t *testing.T) {
			accepter.requests = nil
			event := draftedEvent()
			mutate(&event)
			subscriber := &MailTaskDraftedSubscriber{service: accepter}
			if got := subscriber.process(context.Background(), event); got != mailTaskDraftedTerminate {
				t.Fatalf("outcome = %v, want mailTaskDraftedTerminate", got)
			}
			if got := len(accepter.requests); got != 0 {
				t.Errorf("Accept calls = %d, want 0 for an invalid event", got)
			}
		})
	}
}

// TestMailTaskDraftedMalformedPayloadFailsToDecode exercises the same
// json.Unmarshal step handle() runs before calling process() -- handle()
// responds to a decode error with msg.Term() (logged and acked, never
// blocking the consumer for redelivery of the same poison payload). This
// asserts the decode step itself actually fails closed on garbage input
// rather than silently producing a zero-value event that might otherwise
// slip past validation.
func TestMailTaskDraftedMalformedPayloadFailsToDecode(t *testing.T) {
	var event mailTaskDraftedEvent
	if err := json.Unmarshal([]byte("not json"), &event); err == nil {
		t.Fatal("expected malformed payload to fail decoding")
	}
}

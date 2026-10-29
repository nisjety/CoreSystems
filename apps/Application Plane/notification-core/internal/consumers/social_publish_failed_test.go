package consumers

import (
	"context"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

// fakeAccepter records Accept calls and lets a test inject an error so the
// subscriber's ack/nak/term policy can be asserted.
type fakeAccepter struct {
	mu       sync.Mutex
	requests []notification.Request
	err      error
}

func (f *fakeAccepter) Accept(_ context.Context, request notification.Request) (*notification.AcceptedRequest, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.err != nil {
		return nil, f.err
	}
	f.requests = append(f.requests, request)
	return &notification.AcceptedRequest{}, nil
}

func (f *fakeAccepter) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.requests)
}

func failedPublishEvent() socialLifecycleEvent {
	return socialLifecycleEvent{
		ID:          "evt-1",
		Type:        "publish_job.failed",
		OrgID:       "org-1",
		PostID:      "post-1",
		JobID:       "job-1",
		ActorUserID: "user-1",
		Data:        map[string]any{"last_error": "linkedin: 401 unauthorized"},
		OccurredAt:  time.Now().UTC(),
	}
}

func TestSocialPublishFailed_ValidEvent_AcceptsOneNotification(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &SocialPublishFailedSubscriber{service: accepter}

	if got := s.process(context.Background(), failedPublishEvent()); got != socialPublishAck {
		t.Fatalf("outcome = %v, want socialPublishAck", got)
	}
	if accepter.count() != 1 {
		t.Fatalf("Accept called %d times, want 1", accepter.count())
	}
	req := accepter.requests[0]
	if req.OrganizationID != "org-1" {
		t.Errorf("organization_id = %q, want org-1", req.OrganizationID)
	}
	if req.Recipient.Kind != notification.RecipientKindUser || req.Recipient.ID != "user-1" {
		t.Errorf("recipient = %#v, want typed user-1", req.Recipient)
	}
	if req.Type != NotificationTypeSocialPublishJobFailed {
		t.Errorf("type = %q, want %q", req.Type, NotificationTypeSocialPublishJobFailed)
	}
	if req.IdempotencyKey != "social-publish-job-failed:job-1" {
		t.Errorf("idempotency_key = %q, want social-publish-job-failed:job-1", req.IdempotencyKey)
	}
	if req.Payload["last_error"] != "linkedin: 401 unauthorized" {
		t.Errorf("last_error not forwarded: %+v", req.Payload)
	}
	if req.Payload["org_id"] != "org-1" || req.Payload["post_id"] != "post-1" {
		t.Errorf("org/post not forwarded: %+v", req.Payload)
	}
}

func TestSocialPublishFailed_MissingActorUserID_Terminates(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &SocialPublishFailedSubscriber{service: accepter}

	ev := failedPublishEvent()
	ev.ActorUserID = ""
	if got := s.process(context.Background(), ev); got != socialPublishTerminate {
		t.Fatalf("outcome = %v, want socialPublishTerminate", got)
	}
	if accepter.count() != 0 {
		t.Errorf("Accept called for an event with no actor_user_id")
	}
}

func TestSocialPublishFailed_MissingJobID_FallsBackToEventID(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &SocialPublishFailedSubscriber{service: accepter}

	ev := failedPublishEvent()
	ev.JobID = ""
	if got := s.process(context.Background(), ev); got != socialPublishAck {
		t.Fatalf("outcome = %v, want socialPublishAck", got)
	}
	if got := accepter.requests[0].IdempotencyKey; got != "social-publish-job-failed:evt-1" {
		t.Errorf("idempotency_key = %q, want fallback to event id", got)
	}
}

func TestSocialPublishFailed_AcceptError_Retries(t *testing.T) {
	accepter := &fakeAccepter{err: fmt.Errorf("db unavailable")}
	s := &SocialPublishFailedSubscriber{service: accepter}

	if got := s.process(context.Background(), failedPublishEvent()); got != socialPublishRetry {
		t.Fatalf("outcome = %v, want socialPublishRetry on a transient Accept failure", got)
	}
}

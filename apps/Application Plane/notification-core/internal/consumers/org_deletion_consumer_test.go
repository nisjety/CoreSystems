package consumers

import (
	"context"
	"fmt"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

// fakeAccepter (from social_publish_failed_test.go) is reused here — same
// package, same test conventions.

func pendingEvent() orgDeletionPendingEvent {
	return orgDeletionPendingEvent{
		OrgID:         "org-1",
		OrgName:       "Aquatiq AS",
		RequestedBy:   "user-owner",
		Deadline:      "2026-08-19T00:00:00Z",
		MemberUserIDs: []string{"user-1", "user-2"},
	}
}

func reminderEvent(daysRemaining int) orgDeletionReminderEvent {
	return orgDeletionReminderEvent{
		OrgID:         "org-1",
		OrgName:       "Aquatiq AS",
		DaysRemaining: daysRemaining,
		MemberUserIDs: []string{"user-1", "user-2"},
	}
}

func cancelledEvent() orgDeletionCancelledEvent {
	return orgDeletionCancelledEvent{
		OrgID:       "org-1",
		OrgName:     "Aquatiq AS",
		CancelledBy: "user-owner",
	}
}

func TestOrgDeletionPending_ValidEvent_AcceptsOnePerMember(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &OrgDeletionSubscriber{service: accepter}

	if got := s.processPending(context.Background(), pendingEvent()); got != orgDeletionAck {
		t.Fatalf("outcome = %v, want orgDeletionAck", got)
	}
	if accepter.count() != 2 {
		t.Fatalf("Accept called %d times, want 2", accepter.count())
	}

	for i, wantUser := range []string{"user-1", "user-2"} {
		req := accepter.requests[i]
		if req.OrganizationID != "org-1" {
			t.Errorf("request[%d].OrganizationID = %q, want org-1", i, req.OrganizationID)
		}
		if req.Recipient.Kind != notification.RecipientKindUser || req.Recipient.ID != wantUser {
			t.Errorf("request[%d].Recipient = %#v, want typed %s", i, req.Recipient, wantUser)
		}
		if req.Type != NotificationTypeOrgDeletionPending {
			t.Errorf("request[%d].Type = %q, want %q", i, req.Type, NotificationTypeOrgDeletionPending)
		}
		if req.Source != orgDeletionSource {
			t.Errorf("request[%d].Source = %q, want %q", i, req.Source, orgDeletionSource)
		}
		wantKey := fmt.Sprintf("org-deletion-pending:org-1:%s:2026-08-19T00:00:00Z", wantUser)
		if req.IdempotencyKey != wantKey {
			t.Errorf("request[%d].IdempotencyKey = %q, want %q", i, req.IdempotencyKey, wantKey)
		}
		if req.Payload["deadline"] != "2026-08-19T00:00:00Z" || req.Payload["org_name"] != "Aquatiq AS" || req.Payload["requested_by"] != "user-owner" {
			t.Errorf("request[%d].Payload missing fields: %+v", i, req.Payload)
		}
	}
}

func TestOrgDeletionPending_MissingOrgID_Terminates(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &OrgDeletionSubscriber{service: accepter}

	ev := pendingEvent()
	ev.OrgID = ""
	if got := s.processPending(context.Background(), ev); got != orgDeletionTerminate {
		t.Fatalf("outcome = %v, want orgDeletionTerminate", got)
	}
	if accepter.count() != 0 {
		t.Errorf("Accept called for an event with no org_id")
	}
}

func TestOrgDeletionPending_MissingDeadline_Terminates(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &OrgDeletionSubscriber{service: accepter}

	ev := pendingEvent()
	ev.Deadline = ""
	if got := s.processPending(context.Background(), ev); got != orgDeletionTerminate {
		t.Fatalf("outcome = %v, want orgDeletionTerminate", got)
	}
	if accepter.count() != 0 {
		t.Errorf("Accept called for an event with no deadline")
	}
}

func TestOrgDeletionPending_AcceptError_Retries(t *testing.T) {
	accepter := &fakeAccepter{err: fmt.Errorf("db unavailable")}
	s := &OrgDeletionSubscriber{service: accepter}

	if got := s.processPending(context.Background(), pendingEvent()); got != orgDeletionRetry {
		t.Fatalf("outcome = %v, want orgDeletionRetry on a transient Accept failure", got)
	}
}

func TestOrgDeletionPending_UnauthorizedRecipient_SkipsWithoutRetry(t *testing.T) {
	accepter := &fakeAccepter{err: notification.ErrRecipientNotAuthorized}
	s := &OrgDeletionSubscriber{service: accepter}

	if got := s.processPending(context.Background(), pendingEvent()); got != orgDeletionAck {
		t.Fatalf("outcome = %v, want orgDeletionAck (permanent per-member failure should not block ack)", got)
	}
}

func TestOrgDeletionReminder_ValidEvent_AcceptsOnePerMember(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &OrgDeletionSubscriber{service: accepter}

	if got := s.processReminder(context.Background(), reminderEvent(7)); got != orgDeletionAck {
		t.Fatalf("outcome = %v, want orgDeletionAck", got)
	}
	if accepter.count() != 2 {
		t.Fatalf("Accept called %d times, want 2", accepter.count())
	}
	req := accepter.requests[0]
	if req.Type != NotificationTypeOrgDeletionReminder {
		t.Errorf("Type = %q, want %q", req.Type, NotificationTypeOrgDeletionReminder)
	}
	if req.Payload["days_remaining"] != 7 {
		t.Errorf("days_remaining = %v, want 7", req.Payload["days_remaining"])
	}
	if req.IdempotencyKey != "org-deletion-reminder:org-1:user-1:7d" {
		t.Errorf("IdempotencyKey = %q, want org-deletion-reminder:org-1:user-1:7d", req.IdempotencyKey)
	}
}

func TestOrgDeletionReminder_OneDayVariant_Accepts(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &OrgDeletionSubscriber{service: accepter}

	if got := s.processReminder(context.Background(), reminderEvent(1)); got != orgDeletionAck {
		t.Fatalf("outcome = %v, want orgDeletionAck", got)
	}
	if accepter.requests[0].IdempotencyKey != "org-deletion-reminder:org-1:user-1:1d" {
		t.Errorf("IdempotencyKey = %q, want org-deletion-reminder:org-1:user-1:1d", accepter.requests[0].IdempotencyKey)
	}
}

func TestOrgDeletionReminder_InvalidDaysRemaining_Terminates(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &OrgDeletionSubscriber{service: accepter}

	ev := reminderEvent(0)
	if got := s.processReminder(context.Background(), ev); got != orgDeletionTerminate {
		t.Fatalf("outcome = %v, want orgDeletionTerminate", got)
	}
	if accepter.count() != 0 {
		t.Errorf("Accept called for an event with days_remaining=0")
	}
}

func TestOrgDeletionCancelled_WithMemberRoster_AcceptsOnePerMember(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &OrgDeletionSubscriber{service: accepter}

	ev := cancelledEvent()
	ev.MemberUserIDs = []string{"user-1", "user-2"}
	if got := s.processCancelled(context.Background(), ev); got != orgDeletionAck {
		t.Fatalf("outcome = %v, want orgDeletionAck", got)
	}
	if accepter.count() != 2 {
		t.Fatalf("Accept called %d times, want 2", accepter.count())
	}
	if accepter.requests[0].Type != NotificationTypeOrgDeletionCancelled {
		t.Errorf("Type = %q, want %q", accepter.requests[0].Type, NotificationTypeOrgDeletionCancelled)
	}
}

func TestOrgDeletionCancelled_NoMemberRoster_FallsBackToCancelledBy(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &OrgDeletionSubscriber{service: accepter}

	// Matches the actual published contract: velion.org.deletion.cancelled
	// carries org_id/org_name/cancelled_by only, no member_user_ids.
	if got := s.processCancelled(context.Background(), cancelledEvent()); got != orgDeletionAck {
		t.Fatalf("outcome = %v, want orgDeletionAck", got)
	}
	if accepter.count() != 1 {
		t.Fatalf("Accept called %d times, want 1 (fallback to cancelled_by)", accepter.count())
	}
	req := accepter.requests[0]
	if req.Recipient.ID != "user-owner" {
		t.Errorf("Recipient.ID = %q, want user-owner (cancelled_by fallback)", req.Recipient.ID)
	}
	if req.IdempotencyKey != "org-deletion-cancelled:org-1:user-owner" {
		t.Errorf("IdempotencyKey = %q, want org-deletion-cancelled:org-1:user-owner", req.IdempotencyKey)
	}
}

func TestOrgDeletionCancelled_MissingOrgID_Terminates(t *testing.T) {
	accepter := &fakeAccepter{}
	s := &OrgDeletionSubscriber{service: accepter}

	ev := cancelledEvent()
	ev.OrgID = ""
	if got := s.processCancelled(context.Background(), ev); got != orgDeletionTerminate {
		t.Fatalf("outcome = %v, want orgDeletionTerminate", got)
	}
	if accepter.count() != 0 {
		t.Errorf("Accept called for an event with no org_id")
	}
}

func TestOrgDeletionPending_RedeliveryIsIdempotentKeyStable(t *testing.T) {
	// At-least-once delivery: processing the same event twice must produce
	// identical idempotency keys per member so notification.Service's
	// dedup-by-key path (not this consumer) is what prevents double-send.
	accepter := &fakeAccepter{}
	s := &OrgDeletionSubscriber{service: accepter}

	ev := pendingEvent()
	s.processPending(context.Background(), ev)
	s.processPending(context.Background(), ev)

	if accepter.count() != 4 {
		t.Fatalf("Accept called %d times, want 4 (2 members x 2 deliveries)", accepter.count())
	}
	if accepter.requests[0].IdempotencyKey != accepter.requests[2].IdempotencyKey {
		t.Errorf("redelivery produced different idempotency keys: %q vs %q", accepter.requests[0].IdempotencyKey, accepter.requests[2].IdempotencyKey)
	}
}

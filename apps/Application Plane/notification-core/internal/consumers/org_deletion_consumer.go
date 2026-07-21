// Package consumers: org-core's Zoom-style 30-day organization soft-delete
// flow. org-core (Control Plane) publishes three lifecycle events while an
// organization moves through soft-delete -> reminder -> purge-or-restore;
// this subscriber turns each one into a per-member user-facing notification
// via service.Accept, the same dispatch path the HTTP API uses.
//
// Modeled on ControlSessionSubscriber (control_session.go): same struct
// shape (js, service, subAcks, constructor, Start/Stop), same
// QueueSubscribe/ManualAck/AckWait wiring. Extended with IdentitySyncSubscriber's
// (identity_sync.go) multi-subject-binding loop since this consumer owns
// three subjects instead of one.
package consumers

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"strings"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

const (
	// Subjects published by org-core's owner-gated GDPR soft-delete HTTP
	// handlers (apps/Control Plane/org-core/internal/http) on the shared
	// velion-nats bus.
	SubjectOrgDeletionPending   = "velion.org.deletion.pending"
	SubjectOrgDeletionReminder  = "velion.org.deletion.reminder"
	SubjectOrgDeletionCancelled = "velion.org.deletion.cancelled"

	// NotificationTypeOrgDeletion* are the notification.Request.Type values
	// the runtime client (Novu workflow ID) and any future email template
	// layer use to distinguish the three lifecycle stages.
	NotificationTypeOrgDeletionPending   = "org_deletion.pending"
	NotificationTypeOrgDeletionReminder  = "org_deletion.reminder"
	NotificationTypeOrgDeletionCancelled = "org_deletion.cancelled"

	orgDeletionSource        = "org-core"
	orgDeletionDurablePrefix = "notification-core-org-deletion"

	// controlSharedStream is the pre-provisioned JetStream stream (owned by
	// audit-core's provisioner, apps/Control Plane/audit-core/internal/provisioner)
	// carrying org-core's org-deletion lifecycle subjects. This subscriber's
	// notification-core-gdpr NATS identity is granted only narrow
	// CONSUMER.INFO/ACK permissions for its own durables — not the broader
	// JS.API.STREAM.NAMES lookup nats.Durable() would trigger without an
	// explicit stream — so it must Bind to the pre-provisioned consumer,
	// matching every sibling GDPR consumer in this fix (documents-api-go,
	// conversation-core-go, quarry-control).
	controlSharedStream = "AQENCIA_CONTROLPLANE"
)

// orgDeletionPendingEvent is the wire shape for velion.org.deletion.pending.
type orgDeletionPendingEvent struct {
	OrgID         string   `json:"org_id"`
	OrgName       string   `json:"org_name"`
	RequestedBy   string   `json:"requested_by"`
	Deadline      string   `json:"deadline"`
	MemberUserIDs []string `json:"member_user_ids"`
}

// orgDeletionReminderEvent is the wire shape for velion.org.deletion.reminder.
// Fired with days_remaining: 7 and again with days_remaining: 1.
type orgDeletionReminderEvent struct {
	OrgID         string   `json:"org_id"`
	OrgName       string   `json:"org_name"`
	DaysRemaining int      `json:"days_remaining"`
	MemberUserIDs []string `json:"member_user_ids"`
}

// orgDeletionCancelledEvent is the wire shape for velion.org.deletion.cancelled.
// The published contract carries only org_id/org_name/cancelled_by — no
// member roster. MemberUserIDs is decoded defensively (omitempty) so a
// future org-core revision that adds a roster fans out automatically
// without a code change here; until then processCancelled falls back to
// notifying cancelled_by alone.
type orgDeletionCancelledEvent struct {
	OrgID         string   `json:"org_id"`
	OrgName       string   `json:"org_name"`
	CancelledBy   string   `json:"cancelled_by"`
	MemberUserIDs []string `json:"member_user_ids,omitempty"`
}

// orgDeletionOutcome mirrors the ack/nak/term decision for one message,
// matching SocialPublishFailedSubscriber's testable-outcome pattern.
type orgDeletionOutcome int

const (
	orgDeletionAck orgDeletionOutcome = iota
	orgDeletionRetry
	orgDeletionTerminate
)

// OrgDeletionSubscriber subscribes to org-core's three org-deletion
// lifecycle subjects and converts each into one notification.Request per
// affected member via service.Accept.
type OrgDeletionSubscriber struct {
	js          nats.JetStreamContext
	service     NotificationAccepter
	subAcks     []*nats.Subscription
	consumerDur string
}

// NewOrgDeletionSubscriber wires the subscriber. js may be nil during local
// dev — the constructor logs and Start becomes a no-op, matching
// ControlSessionSubscriber's fail-open shape.
func NewOrgDeletionSubscriber(js nats.JetStreamContext, svc NotificationAccepter) *OrgDeletionSubscriber {
	return &OrgDeletionSubscriber{
		js:          js,
		service:     svc,
		consumerDur: orgDeletionDurablePrefix,
	}
}

// Start binds all three JetStream subscriptions. Idempotent — calling twice
// replaces the previous handlers. Returns an error only on infrastructure
// failures; a missing js or service is a logged no-op.
func (s *OrgDeletionSubscriber) Start(ctx context.Context) error {
	if s == nil {
		return nil
	}
	if s.js == nil {
		log.Printf("consumers/org-deletion: JetStream not configured, skipping subscription")
		return nil
	}
	if s.service == nil {
		log.Printf("consumers/org-deletion: notification service not configured, skipping subscription")
		return nil
	}

	bindings := []struct {
		subject string
		durable string
		handler nats.MsgHandler
	}{
		{SubjectOrgDeletionPending, s.consumerDur + "-pending", s.handlePendingMsg(ctx)},
		{SubjectOrgDeletionReminder, s.consumerDur + "-reminder", s.handleReminderMsg(ctx)},
		{SubjectOrgDeletionCancelled, s.consumerDur + "-cancelled", s.handleCancelledMsg(ctx)},
	}

	for _, binding := range bindings {
		sub, err := s.js.QueueSubscribe(
			binding.subject,
			binding.durable,
			binding.handler,
			nats.Bind(controlSharedStream, binding.durable),
			nats.ManualAck(),
		)
		if err != nil {
			return fmt.Errorf("subscribe %s: %w", binding.subject, err)
		}
		s.subAcks = append(s.subAcks, sub)
		log.Printf("consumers/org-deletion: subscribed to %s", binding.subject)
	}

	return nil
}

// Stop drains the subscriptions. Safe to call multiple times.
func (s *OrgDeletionSubscriber) Stop() {
	if s == nil {
		return
	}
	for _, sub := range s.subAcks {
		if err := sub.Drain(); err != nil {
			log.Printf("consumers/org-deletion: drain error: %v", err)
		}
	}
	s.subAcks = nil
}

func (s *OrgDeletionSubscriber) handlePendingMsg(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		var event orgDeletionPendingEvent
		if err := json.Unmarshal(msg.Data, &event); err != nil {
			log.Printf("consumers/org-deletion: bad payload on %s: %v", msg.Subject, err)
			_ = msg.Term() // poison message — don't redeliver
			return
		}
		s.finish(msg, s.processPending(ctx, event))
	}
}

func (s *OrgDeletionSubscriber) handleReminderMsg(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		var event orgDeletionReminderEvent
		if err := json.Unmarshal(msg.Data, &event); err != nil {
			log.Printf("consumers/org-deletion: bad payload on %s: %v", msg.Subject, err)
			_ = msg.Term()
			return
		}
		s.finish(msg, s.processReminder(ctx, event))
	}
}

func (s *OrgDeletionSubscriber) handleCancelledMsg(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		var event orgDeletionCancelledEvent
		if err := json.Unmarshal(msg.Data, &event); err != nil {
			log.Printf("consumers/org-deletion: bad payload on %s: %v", msg.Subject, err)
			_ = msg.Term()
			return
		}
		s.finish(msg, s.processCancelled(ctx, event))
	}
}

// finish maps an outcome to the ack/nak/term call on the underlying message.
func (s *OrgDeletionSubscriber) finish(msg *nats.Msg, outcome orgDeletionOutcome) {
	switch outcome {
	case orgDeletionRetry:
		if err := msg.Nak(); err != nil {
			log.Printf("consumers/org-deletion: nak failed: %v", err)
		}
	case orgDeletionTerminate:
		_ = msg.Term()
	default:
		if err := msg.Ack(); err != nil {
			log.Printf("consumers/org-deletion: ack failed: %v", err)
		}
	}
}

// processPending maps velion.org.deletion.pending to one notification.Request
// per member. Testable without NATS.
func (s *OrgDeletionSubscriber) processPending(ctx context.Context, event orgDeletionPendingEvent) orgDeletionOutcome {
	orgID := strings.TrimSpace(event.OrgID)
	deadline := strings.TrimSpace(event.Deadline)
	if orgID == "" || deadline == "" {
		log.Printf("consumers/org-deletion: pending event missing org_id or deadline, terminating")
		return orgDeletionTerminate
	}

	return s.notifyMembers(ctx, orgID, event.MemberUserIDs, NotificationTypeOrgDeletionPending,
		func(userID string) string {
			return fmt.Sprintf("org-deletion-pending:%s:%s:%s", orgID, userID, deadline)
		},
		func(userID string) map[string]any {
			return map[string]any{
				"org_id":       orgID,
				"org_name":     event.OrgName,
				"requested_by": event.RequestedBy,
				"deadline":     deadline,
			}
		},
	)
}

// processReminder maps velion.org.deletion.reminder to one notification.Request
// per member. Fired with days_remaining 7 and 1 today; any positive value is
// accepted so a future additional reminder cadence isn't poison-terminated.
func (s *OrgDeletionSubscriber) processReminder(ctx context.Context, event orgDeletionReminderEvent) orgDeletionOutcome {
	orgID := strings.TrimSpace(event.OrgID)
	if orgID == "" || event.DaysRemaining <= 0 {
		log.Printf("consumers/org-deletion: reminder event missing org_id or invalid days_remaining=%d, terminating", event.DaysRemaining)
		return orgDeletionTerminate
	}

	return s.notifyMembers(ctx, orgID, event.MemberUserIDs, NotificationTypeOrgDeletionReminder,
		func(userID string) string {
			return fmt.Sprintf("org-deletion-reminder:%s:%s:%dd", orgID, userID, event.DaysRemaining)
		},
		func(userID string) map[string]any {
			return map[string]any{
				"org_id":         orgID,
				"org_name":       event.OrgName,
				"days_remaining": event.DaysRemaining,
			}
		},
	)
}

// processCancelled maps velion.org.deletion.cancelled to one
// notification.Request per member. The published contract carries no
// member roster, so this falls back to notifying cancelled_by alone unless
// member_user_ids is present (forward-compatible with a future org-core
// revision).
func (s *OrgDeletionSubscriber) processCancelled(ctx context.Context, event orgDeletionCancelledEvent) orgDeletionOutcome {
	orgID := strings.TrimSpace(event.OrgID)
	if orgID == "" {
		log.Printf("consumers/org-deletion: cancelled event missing org_id, terminating")
		return orgDeletionTerminate
	}

	recipients := event.MemberUserIDs
	if len(recipients) == 0 {
		if actor := strings.TrimSpace(event.CancelledBy); actor != "" {
			recipients = []string{actor}
		}
	}

	return s.notifyMembers(ctx, orgID, recipients, NotificationTypeOrgDeletionCancelled,
		func(userID string) string {
			return fmt.Sprintf("org-deletion-cancelled:%s:%s", orgID, userID)
		},
		func(userID string) map[string]any {
			return map[string]any{
				"org_id":       orgID,
				"org_name":     event.OrgName,
				"cancelled_by": event.CancelledBy,
			}
		},
	)
}

// notifyMembers calls service.Accept once per non-empty member id, scoped
// strictly to orgID (never any other organization's rows/recipients — org_id
// always comes from the event that is being processed). A per-member
// permanent failure (unresolvable recipient / validation error) is logged
// and skipped without blocking the rest of the fan-out. Any other error is
// treated as retryable: the whole message is redelivered, which is safe
// because every member's idempotency key is deterministic — members already
// accepted on a prior delivery resume as already-submitted rather than
// double-notifying.
func (s *OrgDeletionSubscriber) notifyMembers(
	ctx context.Context,
	orgID string,
	memberIDs []string,
	notificationType string,
	idempotencyKey func(userID string) string,
	payload func(userID string) map[string]any,
) orgDeletionOutcome {
	if len(memberIDs) == 0 {
		log.Printf("consumers/org-deletion: %s event for org %s has no member_user_ids, nothing to notify", notificationType, orgID)
		return orgDeletionAck
	}

	retry := false
	for _, rawUserID := range memberIDs {
		userID := strings.TrimSpace(rawUserID)
		if userID == "" {
			continue
		}

		req := notification.Request{
			OrganizationID: orgID,
			IdempotencyKey: idempotencyKey(userID),
			Recipient: notification.Recipient{
				Kind: notification.RecipientKindUser,
				ID:   userID,
			},
			Type:    notificationType,
			Source:  orgDeletionSource,
			Payload: payload(userID),
		}

		if _, err := s.service.Accept(ctx, req); err != nil {
			if errors.Is(err, notification.ErrRecipientNotAuthorized) || notification.IsValidationError(err) {
				log.Printf("consumers/org-deletion: skipping member %s (org %s, type %s): %v", userID, orgID, notificationType, err)
				continue
			}
			log.Printf("consumers/org-deletion: notification.Accept failed for member %s (org %s, type %s): %v", userID, orgID, notificationType, err)
			retry = true
			continue
		}
	}

	if retry {
		return orgDeletionRetry
	}
	return orgDeletionAck
}

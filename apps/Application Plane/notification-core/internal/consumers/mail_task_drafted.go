// mail_task_drafted.go converts the gateway's content-free
// verevon.application.mail_task.drafted completion event into a single
// in-app (default; email off by default, migration 014) notification for the
// mail-task alias owner. Chat-wave2 mail-task spec §3.5, owner ruling
// 2026-09-28: Postmark inbound, self-forward only.
//
// Deliberately NOT a follower fan-out like
// ConversationFollowedMessageSubscriber above: the recipient is always
// exactly the event's own user_id -- the person who forwarded the email --
// never anyone else.
//
// Per mail-task spec §7.4: if the general "background task with completion
// notice" binding (Wave 3) lands first, this scoped consumer becomes
// redundant and should be deleted in favor of it rather than kept alongside.
package consumers

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"time"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

const (
	// SubjectMailTaskDrafted is published by the Frontend Plane gateway
	// (apps/gateway/src/domains/mail_task.rs) once a mail-task run's turn
	// completes.
	SubjectMailTaskDrafted = "verevon.application.mail_task.drafted"

	// NotificationTypeMailTaskDrafted is also the preference key used by the
	// notification profile. In-app by default, email off by default
	// (migration 014).
	NotificationTypeMailTaskDrafted = "chat.mail_task_drafted"

	mailTaskDraftedSource  = "verevon-gateway"
	mailTaskDraftedDurable = "notification-core-mail-task-drafted"
	mailTaskDraftedStream  = "VEREVON_APPLICATION"
)

// mailTaskDraftedEvent declares only the fields the subscriber needs. It is
// content-free by construction -- no subject, body, sender, or attachment
// text ever crosses this event (see the gateway's own
// MailTaskDraftedEvent doc comment, which this mirrors field for field).
type mailTaskDraftedEvent struct {
	ID         string    `json:"id"`
	Type       string    `json:"type"`
	OrgID      string    `json:"org_id"`
	UserID     string    `json:"user_id"`
	ThreadID   string    `json:"thread_id"`
	OccurredAt time.Time `json:"occurred_at"`
}

// MailTaskDraftedSubscriber accepts the content-free in-app notification
// request through notification.Service, mirroring
// ConversationFollowedMessageSubscriber's shape exactly except for the
// single-recipient (not fan-out) dispatch.
type MailTaskDraftedSubscriber struct {
	js          nats.JetStreamContext
	service     NotificationAccepter
	subAcks     []*nats.Subscription
	consumerDur string
}

func NewMailTaskDraftedSubscriber(js nats.JetStreamContext, svc NotificationAccepter) *MailTaskDraftedSubscriber {
	return &MailTaskDraftedSubscriber{
		js:          js,
		service:     svc,
		consumerDur: mailTaskDraftedDurable,
	}
}

func (s *MailTaskDraftedSubscriber) Start(ctx context.Context) error {
	if s == nil {
		return nil
	}
	if s.js == nil {
		log.Printf("consumers/mail-task-drafted: JetStream not configured, skipping subscription")
		return nil
	}
	if s.service == nil {
		log.Printf("consumers/mail-task-drafted: notification service not configured, skipping subscription")
		return nil
	}

	// Bind to the provisioned consumer, same reasoning as
	// ConversationFollowedMessageSubscriber.Start: notification-core's NATS
	// principal cannot discover streams, only bind its own pre-provisioned
	// durable.
	sub, err := s.js.QueueSubscribe(
		SubjectMailTaskDrafted,
		s.consumerDur,
		s.handle(ctx),
		nats.Bind(mailTaskDraftedStream, s.consumerDur),
		nats.ManualAck(),
	)
	if err != nil {
		return fmt.Errorf("subscribe %s: %w", SubjectMailTaskDrafted, err)
	}
	s.subAcks = append(s.subAcks, sub)
	log.Printf("consumers/mail-task-drafted: subscribed to %s", SubjectMailTaskDrafted)
	return nil
}

func (s *MailTaskDraftedSubscriber) Stop() {
	if s == nil {
		return
	}
	for _, sub := range s.subAcks {
		if err := sub.Drain(); err != nil {
			log.Printf("consumers/mail-task-drafted: drain error: %v", err)
		}
	}
	s.subAcks = nil
}

type mailTaskDraftedOutcome int

const (
	mailTaskDraftedAck mailTaskDraftedOutcome = iota
	mailTaskDraftedRetry
	mailTaskDraftedTerminate
)

func (s *MailTaskDraftedSubscriber) handle(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		var event mailTaskDraftedEvent
		if err := json.Unmarshal(msg.Data, &event); err != nil {
			// A malformed event is logged and acked, never blocking the
			// consumer (spec §4: "malformed event is logged and acked
			// (never blocks the consumer)").
			log.Printf("consumers/mail-task-drafted: bad payload on %s: %v", msg.Subject, err)
			_ = msg.Term()
			return
		}

		switch s.process(ctx, event) {
		case mailTaskDraftedRetry:
			if err := msg.Nak(); err != nil {
				log.Printf("consumers/mail-task-drafted: nak failed: %v", err)
			}
		case mailTaskDraftedTerminate:
			_ = msg.Term()
		default:
			if err := msg.Ack(); err != nil {
				log.Printf("consumers/mail-task-drafted: ack failed: %v", err)
			}
		}
	}
}

// process accepts exactly one notification for the event's own user_id --
// never a follower list. Deterministic per-event idempotency (keyed by the
// event id) makes a JetStream at-least-once redelivery safe to retry.
func (s *MailTaskDraftedSubscriber) process(ctx context.Context, event mailTaskDraftedEvent) mailTaskDraftedOutcome {
	validOrg := validConversationIdentifier(event.OrgID)
	validUser := validConversationIdentifier(event.UserID)
	validThread := validConversationIdentifier(event.ThreadID)
	hasEventID := event.ID != "" // no fixed shape is mandated on this field; only non-empty matters
	if !validOrg || !validUser || !validThread || !hasEventID || event.Type != "mail_task_drafted" {
		log.Printf("consumers/mail-task-drafted: invalid event %q, terminating", event.ID)
		return mailTaskDraftedTerminate
	}

	req := notification.Request{
		OrganizationID: event.OrgID,
		IdempotencyKey: fmt.Sprintf("mail-task-drafted:%s", event.ID),
		Recipient:      notification.Recipient{Kind: notification.RecipientKindUser, ID: event.UserID},
		Type:           NotificationTypeMailTaskDrafted,
		Source:         mailTaskDraftedSource,
		Payload: map[string]any{
			"title":     "Email task drafted",
			"body":      "An email you forwarded to your task address produced a drafted answer.",
			"cta_label": "Open thread",
			"cta_href":  "/chat/" + event.ThreadID,
			"thread_id": event.ThreadID,
		},
	}
	if _, err := s.service.Accept(ctx, req); err != nil {
		if errors.Is(err, notification.ErrRecipientNotAuthorized) || notification.IsValidationError(err) {
			log.Printf("consumers/mail-task-drafted: skipping recipient %s for org %s: %v", event.UserID, event.OrgID, err)
			return mailTaskDraftedAck
		}
		log.Printf("consumers/mail-task-drafted: notification.Accept failed for recipient %s (org %s): %v", event.UserID, event.OrgID, err)
		return mailTaskDraftedRetry
	}
	return mailTaskDraftedAck
}

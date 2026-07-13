// Package consumers wires NATS topics to notification-core's notification
// service. G14: subscribe to Control Session events emitted by the repurposed
// CP session-core (ADR 0002) and dispatch user-facing notifications.
//
// Note: renamed from `subscribers` to `consumers` in U5-2 (ui-ux-velion-gap.md
// §10) so the new `subscribers` package can own Novu subscriber identity.
package consumers

import (
	"context"
	"encoding/json"
	"fmt"
	"log"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

const (
	// SubjectAppSessionEntitlementsChanged is published by CP session-core
	// (`internal/nats/shared_publisher.go::PublishAppSessionEntitlementsChanged`)
	// whenever a user's Control Session aggregate is refreshed (plan upgrade,
	// org switch, billing webhook ack).
	SubjectAppSessionEntitlementsChanged = "app.session.entitlements_changed"

	// NotificationTypeEntitlementsChanged is the notification.Request.Type the
	// runtime client uses to template the user-facing message (toast / email).
	NotificationTypeEntitlementsChanged = "control_session.entitlements_changed"
)

// EntitlementsChangedEvent is the wire shape published by session-core.
type EntitlementsChangedEvent struct {
	UserID    string `json:"user_id"`
	OrgID     string `json:"org_id"`
	Timestamp string `json:"timestamp"`
}

// ControlSessionSubscriber subscribes to `app.session.*` and converts each
// event into a notification.Request via service.Accept.
type ControlSessionSubscriber struct {
	js          nats.JetStreamContext
	service     *notification.Service
	subAcks     []*nats.Subscription
	consumerDur string // durable name prefix
}

// NewControlSessionSubscriber wires the subscriber. js may be nil during
// local dev — the constructor logs and the Start method becomes a no-op.
func NewControlSessionSubscriber(js nats.JetStreamContext, svc *notification.Service) *ControlSessionSubscriber {
	return &ControlSessionSubscriber{
		js:          js,
		service:     svc,
		consumerDur: "notification-core-control-session",
	}
}

// Start binds the JetStream subscription. Idempotent — calling twice replaces
// the previous handler. Returns an error only on infrastructure failures;
// a missing js or service is a logged no-op.
func (cs *ControlSessionSubscriber) Start(ctx context.Context) error {
	if cs == nil {
		return nil
	}
	if cs.js == nil {
		log.Printf("subscribers/control-session: JetStream not configured, skipping subscription")
		return nil
	}
	if cs.service == nil {
		log.Printf("subscribers/control-session: notification service not configured, skipping subscription")
		return nil
	}

	sub, err := cs.js.QueueSubscribe(
		SubjectAppSessionEntitlementsChanged,
		cs.consumerDur,
		cs.handleEntitlementsChanged(ctx),
		nats.Durable(cs.consumerDur),
		nats.ManualAck(),
		nats.AckWait(30_000_000_000), // 30s — large enough for downstream dispatch
	)
	if err != nil {
		return fmt.Errorf("subscribe %s: %w", SubjectAppSessionEntitlementsChanged, err)
	}
	cs.subAcks = append(cs.subAcks, sub)
	log.Printf("subscribers/control-session: subscribed to %s", SubjectAppSessionEntitlementsChanged)
	return nil
}

// Stop drains the subscriptions. Safe to call multiple times.
func (cs *ControlSessionSubscriber) Stop() {
	if cs == nil {
		return
	}
	for _, s := range cs.subAcks {
		if err := s.Drain(); err != nil {
			log.Printf("subscribers/control-session: drain error: %v", err)
		}
	}
	cs.subAcks = nil
}

func (cs *ControlSessionSubscriber) handleEntitlementsChanged(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		var event EntitlementsChangedEvent
		if err := json.Unmarshal(msg.Data, &event); err != nil {
			log.Printf("subscribers/control-session: bad payload on %s: %v", msg.Subject, err)
			_ = msg.Term() // poison message — don't redeliver
			return
		}

		if event.UserID == "" || event.OrgID == "" {
			log.Printf("subscribers/control-session: event missing user_id or org_id, terminating")
			_ = msg.Term()
			return
		}

		req := notification.Request{
			OrganizationID: event.OrgID,
			IdempotencyKey: fmt.Sprintf("control-session-entitlements:%s:%s:%s", event.OrgID, event.UserID, event.Timestamp),
			Recipient: notification.Recipient{
				Kind: notification.RecipientKindUser,
				ID:   event.UserID,
			},
			Type:   NotificationTypeEntitlementsChanged,
			Source: "control-session",
			Payload: map[string]any{
				"user_id":   event.UserID,
				"org_id":    event.OrgID,
				"timestamp": event.Timestamp,
			},
		}

		if _, err := cs.service.Accept(ctx, req); err != nil {
			log.Printf("subscribers/control-session: notification.Accept failed for user %s: %v", event.UserID, err)
			// Negative ack so JetStream redelivers; transient repo / runtime
			// failures shouldn't drop the event.
			_ = msg.Nak()
			return
		}

		if err := msg.Ack(); err != nil {
			log.Printf("subscribers/control-session: ack failed: %v", err)
		}
	}
}

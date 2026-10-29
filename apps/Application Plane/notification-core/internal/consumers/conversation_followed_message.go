// Package consumers converts the Application-Plane-local conversation event
// emitted after an inbound message is stored into one privacy-preserving,
// governed notification per operator who follows that conversation.
package consumers

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

const (
	// SubjectConversationMessageReceived is published by conversation-core
	// after it has durably stored an inbound message. The event's follower id
	// projection is selected by conversation-core; this consumer never queries
	// conversation storage directly.
	SubjectConversationMessageReceived = "verevon.application.conversation.message.received"

	// NotificationTypeConversationFollowedMessage is also the preference key
	// used by the notification profile. It is in-app by default and email-off
	// by default (migration 009).
	NotificationTypeConversationFollowedMessage = "inbox.conversation_followed_message"

	conversationFollowSource  = "conversation-core"
	conversationFollowDurable = "notification-core-conversation-followed-message"
	conversationFollowStream  = "VEREVON_APPLICATION"
	maxConversationFollowers  = 50
)

var conversationFollowerIDPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`)

// conversationLifecycleEvent declares only fields the subscriber needs.
// Its Data map deliberately remains opaque: the consumer reads follower ids
// only and never inspects message body, participant, provider, or metadata.
type conversationLifecycleEvent struct {
	ID             string         `json:"id"`
	Type           string         `json:"type"`
	OrgID          string         `json:"org_id"`
	ConversationID string         `json:"conversation_id"`
	MessageID      string         `json:"message_id"`
	Data           map[string]any `json:"data"`
	OccurredAt     time.Time      `json:"occurred_at"`
}

// ConversationFollowedMessageSubscriber accepts the content-free in-app
// notification request through notification.Service, preserving its existing
// organization-membership and user-preference checks.
type ConversationFollowedMessageSubscriber struct {
	js          nats.JetStreamContext
	service     NotificationAccepter
	subAcks     []*nats.Subscription
	consumerDur string
}

func NewConversationFollowedMessageSubscriber(js nats.JetStreamContext, svc NotificationAccepter) *ConversationFollowedMessageSubscriber {
	return &ConversationFollowedMessageSubscriber{
		js:          js,
		service:     svc,
		consumerDur: conversationFollowDurable,
	}
}

func (s *ConversationFollowedMessageSubscriber) Start(ctx context.Context) error {
	if s == nil {
		return nil
	}
	if s.js == nil {
		log.Printf("consumers/conversation-followed-message: JetStream not configured, skipping subscription")
		return nil
	}
	if s.service == nil {
		log.Printf("consumers/conversation-followed-message: notification service not configured, skipping subscription")
		return nil
	}

	// Bind to the provisioned consumer instead of asking JetStream to discover
	// a stream. notification-core's NATS principal intentionally cannot call
	// $JS.API.STREAM.NAMES; it has only this durable's INFO/ACK permissions.
	sub, err := s.js.QueueSubscribe(
		SubjectConversationMessageReceived,
		s.consumerDur,
		s.handle(ctx),
		nats.Bind(conversationFollowStream, s.consumerDur),
		nats.ManualAck(),
	)
	if err != nil {
		return fmt.Errorf("subscribe %s: %w", SubjectConversationMessageReceived, err)
	}
	s.subAcks = append(s.subAcks, sub)
	log.Printf("consumers/conversation-followed-message: subscribed to %s", SubjectConversationMessageReceived)
	return nil
}

func (s *ConversationFollowedMessageSubscriber) Stop() {
	if s == nil {
		return
	}
	for _, sub := range s.subAcks {
		if err := sub.Drain(); err != nil {
			log.Printf("consumers/conversation-followed-message: drain error: %v", err)
		}
	}
	s.subAcks = nil
}

type conversationFollowOutcome int

const (
	conversationFollowAck conversationFollowOutcome = iota
	conversationFollowRetry
	conversationFollowTerminate
)

func (s *ConversationFollowedMessageSubscriber) handle(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		var event conversationLifecycleEvent
		if err := json.Unmarshal(msg.Data, &event); err != nil {
			log.Printf("consumers/conversation-followed-message: bad payload on %s: %v", msg.Subject, err)
			_ = msg.Term()
			return
		}

		switch s.process(ctx, event) {
		case conversationFollowRetry:
			if err := msg.Nak(); err != nil {
				log.Printf("consumers/conversation-followed-message: nak failed: %v", err)
			}
		case conversationFollowTerminate:
			_ = msg.Term()
		default:
			if err := msg.Ack(); err != nil {
				log.Printf("consumers/conversation-followed-message: ack failed: %v", err)
			}
		}
	}
}

// process accepts exactly one generic notification per valid unique follower.
// Permanent membership/validation failures affect only that former or invalid
// follower. Transient failures retry the whole source event; deterministic
// per-recipient idempotency makes that retry safe.
func (s *ConversationFollowedMessageSubscriber) process(ctx context.Context, event conversationLifecycleEvent) conversationFollowOutcome {
	orgID := strings.TrimSpace(event.OrgID)
	conversationID := strings.TrimSpace(event.ConversationID)
	messageID := strings.TrimSpace(event.MessageID)
	if !validConversationIdentifier(orgID) || !validConversationIdentifier(conversationID) || !validConversationIdentifier(messageID) || event.Type != "message.received" {
		log.Printf("consumers/conversation-followed-message: invalid event %q, terminating", event.ID)
		return conversationFollowTerminate
	}

	followerIDs := normalizedFollowerIDs(event.Data["follower_user_ids"])
	if len(followerIDs) == 0 {
		return conversationFollowAck
	}

	retry := false
	for _, userID := range followerIDs {
		req := notification.Request{
			OrganizationID: orgID,
			IdempotencyKey: fmt.Sprintf("conversation-followed-message:%s:%s:%s:%s", orgID, conversationID, messageID, userID),
			Recipient:      notification.Recipient{Kind: notification.RecipientKindUser, ID: userID},
			Type:           NotificationTypeConversationFollowedMessage,
			Source:         conversationFollowSource,
			Payload: map[string]any{
				"title":           "New activity in a followed conversation",
				"body":            "A customer sent a new message in a conversation you follow.",
				"cta_label":       "Open Inbox",
				"cta_href":        "/inbox",
				"conversation_id": conversationID,
				"message_id":      messageID,
			},
		}
		if _, err := s.service.Accept(ctx, req); err != nil {
			if errors.Is(err, notification.ErrRecipientNotAuthorized) || notification.IsValidationError(err) {
				log.Printf("consumers/conversation-followed-message: skipping follower %s for org %s: %v", userID, orgID, err)
				continue
			}
			log.Printf("consumers/conversation-followed-message: notification.Accept failed for follower %s (org %s): %v", userID, orgID, err)
			retry = true
		}
	}
	if retry {
		return conversationFollowRetry
	}
	return conversationFollowAck
}

func validConversationIdentifier(value string) bool {
	return conversationFollowerIDPattern.MatchString(value)
}

func normalizedFollowerIDs(value any) []string {
	values, ok := value.([]any)
	if !ok {
		if stringsSlice, stringsOK := value.([]string); stringsOK {
			values = make([]any, len(stringsSlice))
			for index, userID := range stringsSlice {
				values[index] = userID
			}
		} else {
			return nil
		}
	}

	unique := make(map[string]struct{}, len(values))
	for _, raw := range values {
		userID, ok := raw.(string)
		userID = strings.TrimSpace(userID)
		if !ok || !validConversationIdentifier(userID) {
			continue
		}
		unique[userID] = struct{}{}
	}
	ids := make([]string, 0, len(unique))
	for userID := range unique {
		ids = append(ids, userID)
	}
	sort.Strings(ids)
	if len(ids) > maxConversationFollowers {
		return ids[:maxConversationFollowers]
	}
	return ids
}

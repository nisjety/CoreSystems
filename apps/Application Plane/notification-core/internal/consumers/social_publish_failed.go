// Package consumers: social-core publish-job failures. G26 (provider
// business modules program): social-core already publishes a job-level
// failure event once every platform attempt in a publish job has run
// (internal/social/service.go::finishJob) — this subscriber turns that into a
// user-facing notification for the person who requested the publish.
package consumers

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"time"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

const (
	// SubjectSocialPublishJobFailed is published by social-core
	// (internal/social/types.go::SubjectPublishJobFailed) on the shared
	// velion-nats bus, captured by social-core's own VELION_APPLICATION
	// stream (velion.application.>) — no new stream is needed here, NATS
	// auto-binds a durable QueueSubscribe to whichever stream already covers
	// the subject.
	SubjectSocialPublishJobFailed = "velion.application.social.publish_job.failed"

	// NotificationTypeSocialPublishJobFailed is the notification.Request.Type
	// the runtime client uses to template the user-facing message.
	NotificationTypeSocialPublishJobFailed = "social.publish_job_failed"
)

// socialLifecycleEvent decodes social-core's LifecycleEvent wire shape
// (internal/social/types.go — snake_case tags). Only the fields this
// subscriber needs are declared; unknown fields are ignored.
type socialLifecycleEvent struct {
	ID          string         `json:"id"`
	Type        string         `json:"type"`
	OrgID       string         `json:"org_id"`
	PostID      string         `json:"post_id,omitempty"`
	JobID       string         `json:"job_id,omitempty"`
	ActorUserID string         `json:"actor_user_id,omitempty"`
	Data        map[string]any `json:"data"`
	OccurredAt  time.Time      `json:"occurred_at"`
}

// NotificationAccepter is the narrow surface this subscriber needs.
// *notification.Service satisfies it.
type NotificationAccepter interface {
	Accept(ctx context.Context, request notification.Request) (*notification.AcceptedRequest, error)
}

// SocialPublishFailedSubscriber subscribes to social-core's publish-job-failed
// lifecycle event and converts it into a notification.Request via
// service.Accept — the same dispatch path the HTTP API uses.
type SocialPublishFailedSubscriber struct {
	js          nats.JetStreamContext
	service     NotificationAccepter
	subAcks     []*nats.Subscription
	consumerDur string
}

func NewSocialPublishFailedSubscriber(js nats.JetStreamContext, svc NotificationAccepter) *SocialPublishFailedSubscriber {
	return &SocialPublishFailedSubscriber{
		js:          js,
		service:     svc,
		consumerDur: "notification-core-social-publish-failed",
	}
}

// Start binds the JetStream subscription. A missing js or service is a
// logged no-op, matching ControlSessionSubscriber's fail-open shape.
func (s *SocialPublishFailedSubscriber) Start(ctx context.Context) error {
	if s == nil {
		return nil
	}
	if s.js == nil {
		log.Printf("consumers/social-publish-failed: JetStream not configured, skipping subscription")
		return nil
	}
	if s.service == nil {
		log.Printf("consumers/social-publish-failed: notification service not configured, skipping subscription")
		return nil
	}

	sub, err := s.js.QueueSubscribe(
		SubjectSocialPublishJobFailed,
		s.consumerDur,
		s.handle(ctx),
		nats.Durable(s.consumerDur),
		nats.ManualAck(),
		nats.AckWait(30*time.Second),
	)
	if err != nil {
		return fmt.Errorf("subscribe %s: %w", SubjectSocialPublishJobFailed, err)
	}
	s.subAcks = append(s.subAcks, sub)
	log.Printf("consumers/social-publish-failed: subscribed to %s", SubjectSocialPublishJobFailed)
	return nil
}

// Stop drains the subscriptions. Safe to call multiple times.
func (s *SocialPublishFailedSubscriber) Stop() {
	if s == nil {
		return
	}
	for _, sub := range s.subAcks {
		if err := sub.Drain(); err != nil {
			log.Printf("consumers/social-publish-failed: drain error: %v", err)
		}
	}
	s.subAcks = nil
}

// socialPublishOutcome mirrors the ack/nak/term decision for one message.
type socialPublishOutcome int

const (
	socialPublishAck socialPublishOutcome = iota
	socialPublishRetry
	socialPublishTerminate
)

func (s *SocialPublishFailedSubscriber) handle(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		var event socialLifecycleEvent
		if err := json.Unmarshal(msg.Data, &event); err != nil {
			log.Printf("consumers/social-publish-failed: bad payload on %s: %v", msg.Subject, err)
			_ = msg.Term() // poison message — don't redeliver
			return
		}

		switch s.process(ctx, event) {
		case socialPublishRetry:
			if err := msg.Nak(); err != nil {
				log.Printf("consumers/social-publish-failed: nak failed: %v", err)
			}
		case socialPublishTerminate:
			_ = msg.Term()
		default:
			if err := msg.Ack(); err != nil {
				log.Printf("consumers/social-publish-failed: ack failed: %v", err)
			}
		}
	}
}

// process maps one social-core publish-job-failed event to a notification
// request and accepts it. Testable without NATS.
func (s *SocialPublishFailedSubscriber) process(ctx context.Context, event socialLifecycleEvent) socialPublishOutcome {
	// The job's requester is the only person with standing to be told their
	// publish failed. Without one there is no honest recipient — terminate
	// rather than guess (e.g. broadcasting to the whole org).
	if event.ActorUserID == "" || event.OrgID == "" {
		log.Printf("consumers/social-publish-failed: event %s has no actor_user_id or org_id, terminating", event.ID)
		return socialPublishTerminate
	}

	lastError, _ := event.Data["last_error"].(string)
	idempotencyKey := "social-publish-job-failed:" + event.JobID
	if event.JobID == "" {
		idempotencyKey = "social-publish-job-failed:" + event.ID
	}

	req := notification.Request{
		OrganizationID: event.OrgID,
		IdempotencyKey: idempotencyKey,
		Recipient: notification.Recipient{
			Kind: notification.RecipientKindUser,
			ID:   event.ActorUserID,
		},
		Type:   NotificationTypeSocialPublishJobFailed,
		Source: "social-core",
		Payload: map[string]any{
			"org_id":      event.OrgID,
			"job_id":      event.JobID,
			"post_id":     event.PostID,
			"last_error":  lastError,
			"occurred_at": event.OccurredAt,
		},
	}

	if _, err := s.service.Accept(ctx, req); err != nil {
		log.Printf("consumers/social-publish-failed: notification.Accept failed for job %s: %v", event.JobID, err)
		return socialPublishRetry
	}
	return socialPublishAck
}

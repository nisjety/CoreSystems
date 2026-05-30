package nats

import (
	"context"
	"fmt"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/rs/zerolog/log"
)

// SharedPublisher handles cross-plane event publishing to velion-nats.
type SharedPublisher struct {
	client *Client
	js     jetstream.JetStream
}

// NewSharedPublisher wraps an existing Client for cross-plane publishing.
func NewSharedPublisher(client *Client) *SharedPublisher {
	return &SharedPublisher{
		client: client,
		js:     client.JetStream(),
	}
}

func (sp *SharedPublisher) Close() {
	if sp != nil && sp.client != nil {
		sp.client.Close()
	}
}

// EnsureStreams creates the VELION_SESSION and VELION_AGENT JetStream streams
// on velion-nats if they don't already exist.
func (sp *SharedPublisher) EnsureStreams(ctx context.Context) error {
	if sp == nil {
		return nil
	}

	streams := []jetstream.StreamConfig{
		{
			Name:        "VELION_SESSION",
			Description: "Session commands and events for Model Plane v2",
			Subjects:    []string{"velion.session.>"},
			Retention:   jetstream.LimitsPolicy,
			MaxAge:      72 * time.Hour,
			Storage:     jetstream.FileStorage,
			Replicas:    1,
			Discard:     jetstream.DiscardOld,
			MaxBytes:    1 << 30, // 1 GB
		},
		{
			Name:        "VELION_AGENT",
			Description: "Agent run events for Model Plane v2",
			Subjects:    []string{"velion.agent.>"},
			Retention:   jetstream.LimitsPolicy,
			MaxAge:      72 * time.Hour,
			Storage:     jetstream.FileStorage,
			Replicas:    1,
			Discard:     jetstream.DiscardOld,
			MaxBytes:    1 << 30,
		},
		// G10/G14: Control Session events for notification-core + convex-core.
		{
			Name:        "APP_SESSION",
			Description: "Control Session events (entitlements, plan, org-switch) — see ADR 0002",
			Subjects:    []string{"app.session.>"},
			Retention:   jetstream.LimitsPolicy,
			MaxAge:      168 * time.Hour, // 7 days
			Storage:     jetstream.FileStorage,
			Replicas:    1,
			Discard:     jetstream.DiscardOld,
			MaxBytes:    1 << 28, // 256 MB — events are tiny
		},
	}

	for _, cfg := range streams {
		_, err := sp.js.CreateOrUpdateStream(ctx, cfg)
		if err != nil {
			return fmt.Errorf("ensure stream %s: %w", cfg.Name, err)
		}
		log.Info().Str("stream", cfg.Name).Msg("JetStream stream ready")
	}
	return nil
}

// PublishSessionCommand publishes a command to the correct plane based on version.
func (sp *SharedPublisher) PublishSessionCommand(ctx context.Context, sessionID, commandType string, payload []byte, version string) error {
	if sp == nil {
		return fmt.Errorf("shared publisher not initialized")
	}

	switch version {
	case "v2":
		// Route to new canonical subjects for Model Plane v2
		subject := fmt.Sprintf("velion.session.%s.command", sessionID)
		msg := map[string]any{
			"type":       commandType,
			"session_id": sessionID,
			"payload":    string(payload),
			"timestamp":  time.Now().UTC().Format(time.RFC3339Nano),
		}
		return sp.publishJS(ctx, subject, msg)

	case "v1":
		// Route to existing Model Plane v1 subjects (compat)
		subject := fmt.Sprintf("aqencia.reasoning.session.%s.command", sessionID)
		msg := map[string]any{
			"type":       commandType,
			"session_id": sessionID,
			"payload":    string(payload),
			"timestamp":  time.Now().UTC().Format(time.RFC3339Nano),
		}
		return sp.publishJS(ctx, subject, msg)

	default:
		return fmt.Errorf("unknown model plane version: %s", version)
	}
}

// SubscribeAgentEvents subscribes to agent run events for a specific session,
// routing to the correct stream based on the model plane version.
func (sp *SharedPublisher) SubscribeAgentEvents(ctx context.Context, sessionID, version string) (jetstream.Consumer, error) {
	if sp == nil {
		return nil, fmt.Errorf("shared publisher not initialized")
	}

	var subject string
	var streamName string

	switch version {
	case "v2":
		subject = fmt.Sprintf("velion.agent.run.%s.event", sessionID)
		streamName = "VELION_AGENT"
	case "v1":
		subject = fmt.Sprintf("aqencia.reasoning.run.%s.event", sessionID)
		streamName = "AQENCIA_REASONING"
	default:
		return nil, fmt.Errorf("unknown version: %s", version)
	}

	consumer, err := sp.js.CreateOrUpdateConsumer(ctx, streamName, jetstream.ConsumerConfig{
		Name:          fmt.Sprintf("session-core-%s-%s", sessionID[:8], version),
		FilterSubject: subject,
		DeliverPolicy: jetstream.DeliverNewPolicy,
		AckPolicy:     jetstream.AckExplicitPolicy,
		MaxDeliver:    3,
	})
	if err != nil {
		return nil, fmt.Errorf("create consumer for %s: %w", subject, err)
	}
	return consumer, nil
}

func (sp *SharedPublisher) publishJS(ctx context.Context, subject string, data any) error {
	payload, err := marshalJSON(data)
	if err != nil {
		return err
	}
	_, err = sp.js.Publish(ctx, subject, payload)
	return err
}

// PublishSessionEvent publishes canonical session events for SSE fan-out.
func (sp *SharedPublisher) PublishSessionEvent(
	ctx context.Context,
	sessionID string,
	sequence int64,
	eventType string,
	payload []byte,
	createdAt time.Time,
) error {
	if sp == nil {
		return fmt.Errorf("shared publisher not initialized")
	}

	subject := fmt.Sprintf("velion.session.%s.event", sessionID)
	msg := map[string]any{
		"session_id": sessionID,
		"sequence":   sequence,
		"event_type": eventType,
		"payload":    string(payload),
		"created_at": createdAt.UTC().Format(time.RFC3339Nano),
	}
	return sp.publishJS(ctx, subject, msg)
}

// SubscribeSessionEvents subscribes to live session events for SSE streaming.
func (sp *SharedPublisher) SubscribeSessionEvents(sessionID string, handler nats.MsgHandler) (*nats.Subscription, error) {
	if sp == nil || sp.client == nil {
		return nil, fmt.Errorf("shared publisher not initialized")
	}
	subject := fmt.Sprintf("velion.session.%s.event", sessionID)
	return sp.client.Subscribe(subject, handler)
}

func (sp *SharedPublisher) JetStream() jetstream.JetStream {
	return sp.js
}

// PublishAppSessionEntitlementsChanged publishes onto the APP_SESSION stream
// when a user's Control Session aggregate has been refreshed (plan, org, or
// entitlements may have changed). G10 / G14: notification-core + convex-core
// subscribe and react.
func (sp *SharedPublisher) PublishAppSessionEntitlementsChanged(ctx context.Context, userID, orgID string) error {
	if sp == nil {
		return fmt.Errorf("shared publisher not initialized")
	}
	subject := "app.session.entitlements_changed"
	msg := map[string]any{
		"user_id":   userID,
		"org_id":    orgID,
		"timestamp": time.Now().UTC().Format(time.RFC3339Nano),
	}
	return sp.publishJS(ctx, subject, msg)
}

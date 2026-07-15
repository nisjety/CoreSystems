package nats

import (
	"context"
	"fmt"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
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

package channel

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"time"

	"github.com/triodelab/model-plane/services/bridge-core/internal/delivery"
)

// WebhookAdapter is a real, functional channel adapter that delivers session
// responses to an external HTTP endpoint (a generic webhook, or a chat-platform
// incoming webhook such as Slack/Discord/Teams, all of which accept a JSON POST
// and require only a URL — no SDK or per-request credentials).
//
// Delivery is durable: Deliver does not block on the network. It enqueues the
// outbound message into a delivery.Store, and a delivery.Worker drains the
// store with retry/backoff and dead-lettering. This gives at-least-once
// delivery that survives transient upstream failures.
//
// Ingest normalises an inbound operator payload into a structured envelope so
// downstream processing (and the synchronous ingest HTTP response) sees a
// consistent shape regardless of channel.
type WebhookAdapter struct {
	channelName string
	destination string
	store       delivery.Store
	maxAttempts int
}

// WebhookConfig configures a WebhookAdapter.
type WebhookConfig struct {
	// ChannelName is the channel key this adapter is registered under (e.g.
	// "web" or "api"). Recorded on each delivery record for observability.
	ChannelName string
	// Destination is the outbound webhook URL. Required.
	Destination string
	// MaxAttempts caps delivery retries before dead-lettering. Defaults to 5.
	MaxAttempts int
}

// NewWebhookAdapter constructs a WebhookAdapter backed by the given outbox
// store. The store must be drained by a delivery.Worker for messages to
// actually be sent.
func NewWebhookAdapter(cfg WebhookConfig, store delivery.Store) (*WebhookAdapter, error) {
	if cfg.Destination == "" {
		return nil, fmt.Errorf("webhook adapter: destination URL is required")
	}
	if store == nil {
		return nil, fmt.Errorf("webhook adapter: store is required")
	}
	if cfg.MaxAttempts <= 0 {
		cfg.MaxAttempts = 5
	}
	if cfg.ChannelName == "" {
		cfg.ChannelName = "webhook"
	}
	return &WebhookAdapter{
		channelName: cfg.ChannelName,
		destination: cfg.Destination,
		store:       store,
		maxAttempts: cfg.MaxAttempts,
	}, nil
}

// inboundEvent is the normalised shape Ingest emits for an inbound payload.
type inboundEvent struct {
	Channel   string          `json:"channel"`
	SessionID string          `json:"session_id"`
	Payload   json.RawMessage `json:"payload"`
	Text      string          `json:"text,omitempty"`
	IngestedAt time.Time      `json:"ingested_at"`
}

// Ingest normalises the raw payload into a structured inbound event. If the
// payload is valid JSON it is embedded verbatim; otherwise it is surfaced as
// the Text field so callers always receive well-formed JSON.
func (a *WebhookAdapter) Ingest(_ context.Context, sessionID string, payload []byte) ([]byte, error) {
	evt := inboundEvent{
		Channel:    a.channelName,
		SessionID:  sessionID,
		IngestedAt: timeNow().UTC(),
	}
	if json.Valid(payload) && len(payload) > 0 {
		evt.Payload = json.RawMessage(payload)
	} else {
		evt.Text = string(payload)
	}
	out, err := json.Marshal(evt)
	if err != nil {
		return nil, fmt.Errorf("webhook ingest: marshal event: %w", err)
	}
	slog.Debug("webhook ingest", "session_id", sessionID, "channel", a.channelName, "payload_len", len(payload))
	return out, nil
}

// Deliver enqueues the response for durable, retried delivery to the configured
// webhook URL. It returns once the record is persisted to the outbox; the
// actual HTTP POST happens asynchronously in the delivery worker.
func (a *WebhookAdapter) Deliver(ctx context.Context, sessionID string, response []byte) error {
	rec, err := a.store.Enqueue(ctx, delivery.Record{
		SessionID:   sessionID,
		Channel:     a.channelName,
		Destination: a.destination,
		Payload:     response,
		MaxAttempts: a.maxAttempts,
	})
	if err != nil {
		return fmt.Errorf("webhook deliver: enqueue: %w", err)
	}
	slog.Debug("webhook delivery enqueued", "id", rec.ID, "session_id", sessionID, "channel", a.channelName)
	return nil
}

// HTTPSender is the delivery.Sender that performs the actual webhook POST. It
// is wired into the delivery.Worker that drains the store the WebhookAdapter
// enqueues into.
type HTTPSender struct {
	client *http.Client
}

// NewHTTPSender constructs an HTTPSender. A nil client uses a default client
// with a 10s timeout.
func NewHTTPSender(client *http.Client) *HTTPSender {
	if client == nil {
		client = &http.Client{Timeout: 10 * time.Second}
	}
	return &HTTPSender{client: client}
}

// Send POSTs the record payload to its destination as application/json. Any
// non-2xx response is treated as a retryable failure.
func (s *HTTPSender) Send(ctx context.Context, rec delivery.Record) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, rec.Destination, bytes.NewReader(rec.Payload))
	if err != nil {
		return fmt.Errorf("build webhook request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Bridge-Session", rec.SessionID)
	req.Header.Set("X-Bridge-Channel", rec.Channel)
	req.Header.Set("X-Bridge-Delivery", rec.ID)

	resp, err := s.client.Do(req)
	if err != nil {
		return fmt.Errorf("webhook POST %s: %w", rec.Destination, err)
	}
	defer func() { _ = resp.Body.Close() }()
	// Drain the body so the connection can be reused.
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 64*1024))

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("webhook POST %s returned status %d", rec.Destination, resp.StatusCode)
	}
	return nil
}

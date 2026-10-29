// Package events publishes shipping-core's notable actions (deliveries,
// AI recommendations) onto the shared cross-plane NATS broker, so
// audit-core's existing wildcard subscription picks them up the same way
// it already does for every other plane's producers — no audit-core
// change needed. Ports integration-corev2's internal/events.Publisher
// pattern verbatim (each Go service in this monorepo is its own module,
// so the type can't be imported directly across planes).
package events

import (
	"context"
	"encoding/json"
	"os"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/nats-io/nats.go"
)

// Event is the envelope every event carries — same shape as
// integration-corev2's, so audit-core's parsing doesn't need a
// shipping-core-specific case.
type Event struct {
	ID             string         `json:"id"`
	Type           string         `json:"type"`
	Source         string         `json:"source"`
	Version        string         `json:"version"`
	OrganizationID string         `json:"organizationId"`
	UserID         string         `json:"userId,omitempty"`
	Data           map[string]any `json:"data,omitempty"`
	CreatedAt      time.Time      `json:"createdAt"`
}

// Publisher is the seam booking/recommend depend on — small enough that
// tests use NoopPublisher instead of a real NATS connection.
type Publisher interface {
	Publish(ctx context.Context, event Event) error
}

// NoopPublisher discards every event. Used when NATS_URL is unset —
// shipping-core keeps working standalone, exactly like
// DataPlaneDocumentsClient's Configured()-gated skip.
type NoopPublisher struct{}

func (NoopPublisher) Publish(context.Context, Event) error { return nil }

// NATSPublisher publishes onto the shared verevon-nats broker.
type NATSPublisher struct {
	conn          *nats.Conn
	subjectPrefix string
	source        string
}

// Config points at the shared NATS broker.
type Config struct {
	URL           string
	Token         string
	SubjectPrefix string
}

// NewConfigFromEnv reads NATS_URL, NATS_TOKEN, and NATS_SUBJECT_PREFIX
// (defaulting the prefix to "shipping-core"). An empty URL means "not
// configured" — callers should fall back to NoopPublisher rather than
// erroring, matching every other optional cross-plane integration in this
// codebase.
func NewConfigFromEnv() Config {
	return Config{
		URL:           envOr("NATS_URL", ""),
		Token:         envOr("NATS_TOKEN", ""),
		SubjectPrefix: envOr("NATS_SUBJECT_PREFIX", "shipping-core"),
	}
}

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

// NewNATSPublisher connects to cfg.URL. Returns an error only on a
// connection failure — callers decide whether that's fatal or a
// fall-back-to-Noop situation (main.go chooses the latter).
func NewNATSPublisher(cfg Config, serviceName string) (*NATSPublisher, error) {
	options := []nats.Option{
		nats.Name(serviceName),
		nats.Timeout(5 * time.Second),
	}
	if cfg.Token != "" {
		// The shared verevon-nats broker enforces single-token authorization
		// (nats-shared.conf), not username/password — same as every other
		// producer on this broker.
		options = append(options, nats.Token(cfg.Token))
	}
	conn, err := nats.Connect(cfg.URL, options...)
	if err != nil {
		return nil, err
	}
	return &NATSPublisher{conn: conn, subjectPrefix: strings.TrimSpace(cfg.SubjectPrefix), source: serviceName}, nil
}

// Publish sends event on subjectPrefix + "." + event.Type. Defaults ID,
// Source, Version, and CreatedAt when the caller left them zero, mirroring
// integration-corev2's Publish.
func (p *NATSPublisher) Publish(_ context.Context, event Event) error {
	if event.ID == "" {
		event.ID = "evt_" + uuid.NewString()
	}
	if event.Source == "" {
		event.Source = p.source
	}
	if event.Version == "" {
		event.Version = "1"
	}
	if event.CreatedAt.IsZero() {
		event.CreatedAt = time.Now().UTC()
	}
	payload, err := json.Marshal(event)
	if err != nil {
		return err
	}
	return p.conn.Publish(p.subject(event.Type), payload)
}

// Close drains the connection.
func (p *NATSPublisher) Close() error {
	if p == nil || p.conn == nil {
		return nil
	}
	return p.conn.Drain()
}

func (p *NATSPublisher) subject(eventType string) string {
	eventType = strings.Trim(eventType, ". ")
	prefix := strings.Trim(p.subjectPrefix, ". ")
	if prefix == "" {
		return eventType
	}
	return prefix + "." + eventType
}

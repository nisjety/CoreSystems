package nats

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
)

// Client wraps NATS connection and JetStream
type Client struct {
	conn *nats.Conn
	js   jetstream.JetStream
}

// Config holds NATS connection configuration
type Config struct {
	URL                  string
	MaxReconnectAttempts int
	ReconnectWait        time.Duration
	Name                 string
	Token                string // Token for token-based authentication
}

// NewClient creates a new NATS client with JetStream support
func NewClient(cfg Config) (*Client, error) {
	opts := []nats.Option{
		nats.Name(cfg.Name),
		nats.CustomInboxPrefix("_INBOX.USER_CONTROL"),
		nats.MaxReconnects(cfg.MaxReconnectAttempts),
		nats.ReconnectWait(cfg.ReconnectWait),
		nats.DisconnectErrHandler(func(nc *nats.Conn, err error) {
			if err != nil {
				log.Printf("⚠️  NATS disconnected: %v", err)
			}
		}),
		nats.ReconnectHandler(func(nc *nats.Conn) {
			log.Printf("🔄 NATS reconnected to %s", nc.ConnectedUrl())
		}),
		nats.ClosedHandler(func(nc *nats.Conn) {
			log.Printf("❌ NATS connection closed")
		}),
	}

	authOptions, err := runtimeAuthOptions(cfg.Token)
	if err != nil {
		return nil, fmt.Errorf("configure NATS authentication: %w", err)
	}
	opts = append(opts, authOptions...)

	// Connect to NATS
	conn, err := nats.Connect(cfg.URL, opts...)
	if err != nil {
		return nil, fmt.Errorf("failed to connect to NATS: %w", err)
	}

	log.Printf("✅ Connected to NATS: %s", cfg.URL)

	// Create JetStream context
	js, err := jetstream.New(conn)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("failed to create JetStream: %w", err)
	}

	return &Client{
		conn: conn,
		js:   js,
	}, nil
}

// Close closes the NATS connection
func (c *Client) Close() {
	if c.conn != nil {
		c.conn.Close()
		log.Println("🔌 NATS connection closed")
	}
}

// IsConnected checks if NATS is connected
func (c *Client) IsConnected() bool {
	return c.conn != nil && c.conn.IsConnected()
}

// Publish publishes a message to a subject
func (c *Client) Publish(subject string, data any) error {
	payload, err := json.Marshal(data)
	if err != nil {
		return fmt.Errorf("failed to marshal data: %w", err)
	}

	if err := c.conn.Publish(subject, payload); err != nil {
		return fmt.Errorf("failed to publish to %s: %w", subject, err)
	}

	log.Printf("📤 Published to %s", subject)
	return nil
}

// PublishJetStream publishes a message to JetStream
func (c *Client) PublishJetStream(ctx context.Context, subject string, data any) error {
	payload, err := json.Marshal(data)
	if err != nil {
		return fmt.Errorf("failed to marshal data: %w", err)
	}

	_, err = c.js.Publish(ctx, subject, payload)
	if err != nil {
		return fmt.Errorf("failed to publish to JetStream %s: %w", subject, err)
	}

	log.Printf("📤 Published to JetStream: %s", subject)
	return nil
}

// PublishJetStreamWithMsgID waits for a PubAck and pins the immutable outbox
// event identity to JetStream's deduplication header.
func (c *Client) PublishJetStreamWithMsgID(ctx context.Context, subject, eventID string, payload []byte) error {
	if eventID == "" {
		return fmt.Errorf("JetStream message ID is required")
	}
	if _, err := c.js.Publish(ctx, subject, payload, jetstream.WithMsgID(eventID)); err != nil {
		return fmt.Errorf("failed to publish to JetStream %s: %w", subject, err)
	}
	log.Printf("📤 Published to JetStream with message ID: %s (%s)", subject, eventID)
	return nil
}

// Subscribe subscribes to a subject with a handler
func (c *Client) Subscribe(subject string, handler func(msg *nats.Msg)) (*nats.Subscription, error) {
	sub, err := c.conn.Subscribe(subject, handler)
	if err != nil {
		return nil, fmt.Errorf("failed to subscribe to %s: %w", subject, err)
	}

	log.Printf("📥 Subscribed to: %s", subject)
	return sub, nil
}

// QueueSubscribe subscribes to a subject with queue group
func (c *Client) QueueSubscribe(subject, queue string, handler func(msg *nats.Msg)) (*nats.Subscription, error) {
	sub, err := c.conn.QueueSubscribe(subject, queue, handler)
	if err != nil {
		return nil, fmt.Errorf("failed to queue subscribe to %s: %w", subject, err)
	}

	log.Printf("📥 Queue subscribed to: %s (queue: %s)", subject, queue)
	return sub, nil
}

// Request sends a request and waits for a response
func (c *Client) Request(subject string, data any, timeout time.Duration) (*nats.Msg, error) {
	payload, err := json.Marshal(data)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal data: %w", err)
	}

	msg, err := c.conn.Request(subject, payload, timeout)
	if err != nil {
		return nil, fmt.Errorf("failed to send request to %s: %w", subject, err)
	}

	return msg, nil
}

// GetConnection returns the underlying NATS connection
func (c *Client) GetConnection() *nats.Conn {
	return c.conn
}

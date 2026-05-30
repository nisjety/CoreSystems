package nats

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"strings"
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

	// Add token authentication if provided
	if cfg.Token != "" {
		opts = append(opts, nats.Token(cfg.Token))
		log.Printf("🔐 Using NATS token authentication")
	}

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
func (c *Client) Publish(subject string, data interface{}) error {
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
func (c *Client) PublishJetStream(ctx context.Context, subject string, data interface{}) error {
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
func (c *Client) Request(subject string, data interface{}, timeout time.Duration) (*nats.Msg, error) {
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

// CreateStream creates or updates a JetStream stream
func (c *Client) CreateStream(ctx context.Context, config jetstream.StreamConfig) error {
	_, err := c.js.CreateStream(ctx, config)
	if err != nil {
		errMsg := strings.ToLower(err.Error())
		if strings.Contains(errMsg, "stream name already in use") ||
			strings.Contains(errMsg, "subjects overlap with an existing stream") ||
			strings.Contains(errMsg, "err_code=10058") ||
			strings.Contains(errMsg, "err_code=10065") {
			log.Printf("📋 Stream %s already exists", config.Name)
			return nil
		}
		return fmt.Errorf("failed to create stream %s: %w", config.Name, err)
	}

	log.Printf("✅ Created JetStream stream: %s", config.Name)
	return nil
}

// CreateConsumer creates a consumer for a stream
func (c *Client) CreateConsumer(ctx context.Context, streamName string, config jetstream.ConsumerConfig) (jetstream.Consumer, error) {
	consumer, err := c.js.CreateConsumer(ctx, streamName, config)
	if err != nil {
		return nil, fmt.Errorf("failed to create consumer for %s: %w", streamName, err)
	}

	log.Printf("✅ Created consumer: %s for stream: %s", config.Name, streamName)
	return consumer, nil
}

// GetJetStream returns the JetStream instance
func (c *Client) GetJetStream() jetstream.JetStream {
	return c.js
}

// GetConnection returns the underlying NATS connection
func (c *Client) GetConnection() *nats.Conn {
	return c.conn
}

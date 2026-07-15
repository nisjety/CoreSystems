package nats

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
)

type Client struct {
	conn *nats.Conn
	js   jetstream.JetStream
}

type Config struct {
	URL   string
	Token string
	Name  string
}

func NewClient(cfg Config) (*Client, error) {
	opts := []nats.Option{
		nats.Name(cfg.Name),
		nats.CustomInboxPrefix("_INBOX.BILLING_CONTROL"),
		nats.MaxReconnects(-1),
		nats.ReconnectWait(2 * time.Second),
	}
	authOptions, err := runtimeAuthOptions(cfg.Token)
	if err != nil {
		return nil, fmt.Errorf("configure nats authentication: %w", err)
	}
	opts = append(opts, authOptions...)

	conn, err := nats.Connect(cfg.URL, opts...)
	if err != nil {
		return nil, fmt.Errorf("connect nats: %w", err)
	}

	js, err := jetstream.New(conn)
	if err != nil {
		conn.Close()
		return nil, fmt.Errorf("jetstream init: %w", err)
	}

	return &Client{conn: conn, js: js}, nil
}

func (c *Client) Close() {
	if c.conn != nil {
		c.conn.Close()
	}
}

func (c *Client) Subscribe(subject string, handler nats.MsgHandler) (*nats.Subscription, error) {
	return c.conn.Subscribe(subject, handler)
}

func (c *Client) Publish(ctx context.Context, subject string, payload map[string]any) error {
	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("marshal event: %w", err)
	}
	if _, err := c.js.Publish(ctx, subject, body); err != nil {
		return fmt.Errorf("publish %s: %w", subject, err)
	}
	return nil
}

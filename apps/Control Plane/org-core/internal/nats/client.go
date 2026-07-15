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
	js   localJetStreamPublisher
}

type Config struct {
	URL   string
	Token string
	Name  string
}

type jetStreamMessagePublisher interface {
	PublishMsg(context.Context, *nats.Msg, ...jetstream.PublishOpt) (*jetstream.PubAck, error)
}

type localJetStreamPublisher interface {
	jetStreamMessagePublisher
	Publish(context.Context, string, []byte, ...jetstream.PublishOpt) (*jetstream.PubAck, error)
}

func NewClient(cfg Config) (*Client, error) {
	opts := []nats.Option{
		nats.Name(cfg.Name),
		nats.CustomInboxPrefix("_INBOX.ORG_CONTROL"),
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

// PublishAudit publishes a durable audit event with a stable logical message
// identity and returns only after JetStream supplies a valid PubAck.
func (c *Client) PublishAudit(ctx context.Context, subject, eventID string, payload map[string]any) error {
	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("marshal audit event: %w", err)
	}
	return publishAuditMessage(ctx, c.js, subject, eventID, body)
}

func publishAuditMessage(
	ctx context.Context,
	publisher jetStreamMessagePublisher,
	subject, eventID string,
	payload []byte,
) error {
	if publisher == nil {
		return fmt.Errorf("JetStream audit publisher unavailable")
	}
	if subject == "" || eventID == "" {
		return fmt.Errorf("audit subject and event ID are required")
	}
	msg := nats.NewMsg(subject)
	msg.Header = nats.Header{}
	msg.Header.Set(nats.MsgIdHdr, eventID)
	msg.Data = append([]byte(nil), payload...)
	ack, err := publisher.PublishMsg(ctx, msg)
	if err != nil {
		return fmt.Errorf("publish audit event %s: %w", eventID, err)
	}
	if ack == nil || ack.Stream == "" || ack.Sequence == 0 {
		return fmt.Errorf("invalid JetStream PubAck for audit event %s", eventID)
	}
	return nil
}

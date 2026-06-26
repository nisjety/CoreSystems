// Package nats wires insight-core's optional JetStream consumer connection. It
// mirrors conversation-core-go/internal/nats (insight-core is a separate Go
// module, so the small client is duplicated rather than imported).
package nats

import (
	"errors"
	"time"

	"github.com/nats-io/nats.go"
)

type Config struct {
	URL   string
	Token string
	Name  string
}

type Client struct {
	Conn *nats.Conn
	JS   nats.JetStreamContext
}

func NewClient(cfg Config) (*Client, error) {
	options := []nats.Option{
		nats.Name(cfg.Name),
		nats.Timeout(5 * time.Second),
	}
	if cfg.Token != "" {
		options = append(options, nats.Token(cfg.Token))
	}
	conn, err := nats.Connect(cfg.URL, options...)
	if err != nil {
		return nil, err
	}
	js, err := conn.JetStream()
	if err != nil {
		conn.Close()
		return nil, err
	}
	return &Client{Conn: conn, JS: js}, nil
}

func (c *Client) Close() {
	if c == nil || c.Conn == nil {
		return
	}
	c.Conn.Close()
}

// EnsureStream provisions a bounded JetStream stream over the given subjects
// when no stream by this name exists yet. It is needed because the Model Plane
// publishes run/approval lifecycle events via CORE NATS (fire-and-forget, no
// stream), so a durable JetStream consumer has nothing to bind to ("no stream
// matches subject"). A JetStream stream additionally captures messages that
// match its subjects regardless of whether the producer used core publish — so
// creating one makes the existing run events durably consumable WITHOUT
// touching the producer, and without disturbing core-NATS subscribers (e.g.
// cost-core on mp.v1.usage.*). Idempotent: an already-present stream is left
// as-is. Bounded retention keeps the model-plane bus storage in check — the
// metrics are persisted to Postgres on consumption; the stream is only transport.
func (c *Client) EnsureStream(name string, subjects []string) error {
	if c == nil || c.JS == nil {
		return nil
	}
	if _, err := c.JS.StreamInfo(name); err == nil {
		return nil // already provisioned
	} else if !errors.Is(err, nats.ErrStreamNotFound) {
		return err
	}
	_, err := c.JS.AddStream(&nats.StreamConfig{
		Name:      name,
		Subjects:  subjects,
		Retention: nats.LimitsPolicy,
		Storage:   nats.FileStorage,
		Discard:   nats.DiscardOld,
		MaxAge:    48 * time.Hour,
		MaxBytes:  64 * 1024 * 1024,
	})
	// A concurrent insight-core replica may have created it between our check
	// and AddStream — tolerate the resulting "already in use" race.
	if err != nil && errors.Is(err, nats.ErrStreamNameAlreadyInUse) {
		return nil
	}
	return err
}

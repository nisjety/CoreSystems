package events

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/nats-io/nats.go"
)

// Publisher publishes JSON-encoded payloads to NATS subjects. The zero value
// is a no-op publisher — useful when NATS_URL is empty in dev environments.
type Publisher struct {
	conn *nats.Conn
}

// ErrPublisherClosed is returned when Publish is called on a closed publisher.
var ErrPublisherClosed = errors.New("events: publisher is closed")

// Connect dials NATS at url. If url is empty, returns a no-op Publisher so
// callers can publish unconditionally without checking config.
func Connect(ctx context.Context, url, clientName string) (*Publisher, error) {
	if url == "" {
		return &Publisher{}, nil
	}

	opts := []nats.Option{
		nats.Name(clientName),
		nats.ReconnectWait(2 * time.Second),
		nats.MaxReconnects(-1),
		nats.Timeout(5 * time.Second),
		nats.PingInterval(30 * time.Second),
	}

	connCh := make(chan struct {
		nc  *nats.Conn
		err error
	}, 1)
	go func() {
		nc, err := nats.Connect(url, opts...)
		connCh <- struct {
			nc  *nats.Conn
			err error
		}{nc, err}
	}()

	select {
	case <-ctx.Done():
		return nil, fmt.Errorf("nats connect cancelled: %w", ctx.Err())
	case res := <-connCh:
		if res.err != nil {
			return nil, fmt.Errorf("nats connect: %w", res.err)
		}
		return &Publisher{conn: res.nc}, nil
	}
}

// Publish marshals payload as JSON and publishes to subject.
func (p *Publisher) Publish(subject string, payload any) error {
	if p == nil || p.conn == nil {
		return nil
	}
	if p.conn.IsClosed() {
		return ErrPublisherClosed
	}
	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("marshal payload: %w", err)
	}
	if err := p.conn.Publish(subject, body); err != nil {
		return fmt.Errorf("publish %s: %w", subject, err)
	}
	return nil
}

// Drain flushes pending messages and closes the connection.
func (p *Publisher) Drain() error {
	if p == nil || p.conn == nil {
		return nil
	}
	return p.conn.Drain()
}

// Healthy reports whether the underlying connection is currently usable.
// A nil/no-op publisher is considered healthy.
func (p *Publisher) Healthy() bool {
	if p == nil || p.conn == nil {
		return true
	}
	return p.conn.IsConnected()
}

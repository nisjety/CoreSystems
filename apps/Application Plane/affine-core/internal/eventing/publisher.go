package eventing

import (
	"context"
	"encoding/json"

	"github.com/nats-io/nats.go"
)

type Publisher struct {
	conn *nats.Conn
}

func NewPublisher(conn *nats.Conn) *Publisher {
	return &Publisher{conn: conn}
}

func (p *Publisher) Publish(ctx context.Context, subject string, payload any) error {
	if p == nil || p.conn == nil {
		return nil
	}
	bytes, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	return p.conn.Publish(subject, bytes)
}

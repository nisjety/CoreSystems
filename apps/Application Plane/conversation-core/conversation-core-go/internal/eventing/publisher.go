package eventing

import (
	"context"
	"encoding/json"

	"github.com/nats-io/nats.go"
)

type Publisher struct {
	js nats.JetStreamContext
}

func NewPublisher(js nats.JetStreamContext) *Publisher {
	return &Publisher{js: js}
}

func (p *Publisher) Publish(ctx context.Context, subject string, payload any) error {
	if p == nil || p.js == nil {
		return nil
	}
	bytes, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	_, err = p.js.Publish(subject, bytes)
	return err
}

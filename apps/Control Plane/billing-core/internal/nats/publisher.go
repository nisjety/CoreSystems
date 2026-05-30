package nats

import (
	"context"
)

type Publisher struct {
	client *Client
}

func NewPublisher(client *Client) *Publisher {
	return &Publisher{client: client}
}

func (p *Publisher) Publish(ctx context.Context, subject string, payload map[string]any) error {
	return p.client.Publish(ctx, subject, payload)
}

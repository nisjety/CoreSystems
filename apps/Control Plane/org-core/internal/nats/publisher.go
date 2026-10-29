package nats

import (
	"context"
	"time"
)

type Publisher struct {
	client *Client
}

func NewPublisher(client *Client) *Publisher {
	return &Publisher{client: client}
}

func (p *Publisher) Publish(ctx context.Context, subject string, data map[string]any) error {
	event := map[string]any{
		"type":      subject,
		"timestamp": time.Now().UTC().Format(time.RFC3339),
		"data":      data,
	}
	return p.client.Publish(ctx, subject, event)
}

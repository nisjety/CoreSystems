package eventtrigger

import (
	"context"
	"errors"
	"fmt"
	"log/slog"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
)

// delivery tells the JetStream handler whether a message is settled or must be redelivered.
type delivery int

const (
	deliveryAck delivery = iota
	deliveryRetry
)

// Consumer records a pending fire for each webhook_received event. It binds the durable the stream owner provisioned,
// with manual ack.
type Consumer struct {
	recorder recorder
}

// NewConsumer builds the consumer over the fire table.
func NewConsumer(pool *pgxpool.Pool) (*Consumer, error) {
	if pool == nil {
		return nil, errors.New("event trigger consumer needs a database")
	}
	return newConsumer(pgStore{pool: pool}), nil
}

func newConsumer(r recorder) *Consumer {
	return &Consumer{recorder: r}
}

// Run binds WebhookDurable on WebhookStream with manual ack and blocks until ctx is cancelled. js must already have the
// durable provisioned: this call binds it and never creates it.
func (c *Consumer) Run(ctx context.Context, js nats.JetStreamContext) error {
	sub, err := js.QueueSubscribe(WebhookReceivedSubject, WebhookDurable, func(msg *nats.Msg) {
		c.handle(ctx, msg)
	}, nats.Bind(WebhookStream, WebhookDurable), nats.ManualAck())
	if err != nil {
		return fmt.Errorf("eventtrigger: bind %s/%s: %w", WebhookStream, WebhookDurable, err)
	}
	slog.Info("event trigger consumer bound", "stream", WebhookStream, "durable", WebhookDurable,
		"subject", WebhookReceivedSubject)
	<-ctx.Done()
	_ = sub.Unsubscribe()
	return nil
}

func (c *Consumer) handle(ctx context.Context, msg *nats.Msg) {
	if c.process(ctx, msg.Data) == deliveryRetry {
		if err := msg.Nak(); err != nil {
			slog.Warn("event trigger consumer: nak failed", "subject", msg.Subject, "error", err)
		}
		return
	}
	if err := msg.Ack(); err != nil {
		slog.Warn("event trigger consumer: ack failed", "subject", msg.Subject, "error", err)
	}
}

// process records one event's fires. A malformed event never becomes valid, so it is acknowledged and dropped; only a
// failed write is retried. It is the testable core: no broker is needed to exercise it.
func (c *Consumer) process(ctx context.Context, payload []byte) delivery {
	event, err := ParseWebhookReceived(payload)
	if err != nil {
		slog.Warn("event trigger consumer dropped a webhook_received event", "error", err)
		return deliveryAck
	}
	recorded, err := c.recorder.record(ctx, event)
	if err != nil {
		slog.Warn("event trigger consumer could not record fires; will retry", "org_id", event.OrgID, "error", err)
		return deliveryRetry
	}
	if recorded > 0 {
		slog.Info("event trigger fires recorded", "count", recorded)
	}
	return deliveryAck
}

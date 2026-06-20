// Package consumers hosts insight-core's in-process JetStream consumers.
//
// DurableConsumer is a thin, reusable scaffold over a nats.JetStreamContext for
// durable push consumers. It mirrors conversation-core-go/internal/consumers
// (insight-core is a separate Go module, so the scaffold is duplicated rather
// than imported) — manual ack + bounded ack-wait + sequential processing.
package consumers

import (
	"log"
	"time"

	"github.com/nats-io/nats.go"
)

type DurableConsumer struct {
	js   nats.JetStreamContext
	name string
	subs []*nats.Subscription
}

func NewDurableConsumer(js nats.JetStreamContext, name string) *DurableConsumer {
	return &DurableConsumer{js: js, name: name}
}

// Bind registers a durable, manually-acked push subscription on subject. The
// durable name doubles as the queue group. Returns an error if the JetStream
// context is missing or the subscribe fails (e.g. the stream does not exist yet).
func (c *DurableConsumer) Bind(subject, durable string, handler nats.MsgHandler) error {
	if c == nil || c.js == nil {
		return nil
	}
	sub, err := c.js.QueueSubscribe(subject, durable, handler,
		nats.Durable(durable),
		nats.ManualAck(),
		nats.AckWait(30*time.Second),
		nats.MaxAckPending(1),
	)
	if err != nil {
		return err
	}
	c.subs = append(c.subs, sub)
	log.Printf("[insight-core/consumers] %s subscribed to %s (durable=%s)", c.name, subject, durable)
	return nil
}

// Stop drains every bound subscription.
func (c *DurableConsumer) Stop() {
	if c == nil {
		return
	}
	for _, sub := range c.subs {
		if sub == nil {
			continue
		}
		if err := sub.Unsubscribe(); err != nil {
			log.Printf("[insight-core/consumers] %s unsubscribe %s: %v", c.name, sub.Subject, err)
		}
	}
	c.subs = nil
}

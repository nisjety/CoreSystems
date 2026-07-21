// Package consumers hosts in-process JetStream consumers for conversation-core.
//
// DurableConsumer is a thin, reusable scaffold over a nats.JetStreamContext for
// durable push consumers. It owns the subscribe / manual-ack / drain
// boilerplate so each handler only writes its own logic. It is modelled on
// notification-core/internal/consumers and is intended to be mirrored by other
// Application-Plane services (insight-core, PR-3) rather than imported across
// module boundaries.
package consumers

import (
	"log"
	"time"

	"github.com/nats-io/nats.go"
)

// DurableConsumer binds one or more durable push subscriptions and drains them
// on shutdown. Each binding uses manual ack + a bounded ack-wait so a handler
// has time to apply side effects before JetStream redelivers, and
// MaxAckPending(1) so a single in-process consumer processes sequentially.
type DurableConsumer struct {
	js   nats.JetStreamContext
	name string
	subs []*nats.Subscription
}

const (
	applicationEventsStream    = "VELION_APPLICATION"
	applicationModelStream     = "VELION_MODEL"
	applicationIngestionStream = "VELION_INGESTION"
	// controlSharedStream carries platform-wide, cross-plane events published
	// by Control-Plane services (e.g. org-core's GDPR erasure fan-out) that
	// are not scoped to the application/model/ingestion namespaces. Mirrors
	// documents-api-go's GDPR subscriber (Data Plane v2) and audit-core's
	// provisioner (ControlSharedStreamName).
	controlSharedStream = "AQENCIA_CONTROLPLANE"
)

func NewDurableConsumer(js nats.JetStreamContext, name string) *DurableConsumer {
	return &DurableConsumer{js: js, name: name}
}

// Bind registers a durable, manually-acked push subscription on subject. The
// durable name doubles as the queue group so the binding is replayable and
// horizontally safe. Returns an error if the JetStream context is missing or
// the subscribe fails (e.g. the stream does not exist yet).
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
	log.Printf("[cc-go/consumers] %s subscribed to %s (durable=%s)", c.name, subject, durable)
	return nil
}

func (c *DurableConsumer) BindProvisioned(subject, stream, durable string, handler nats.MsgHandler) error {
	if c == nil || c.js == nil {
		return nil
	}
	sub, err := c.js.QueueSubscribe(subject, durable, handler,
		nats.Bind(stream, durable),
		nats.ManualAck(),
	)
	if err != nil {
		return err
	}
	c.subs = append(c.subs, sub)
	log.Printf("[cc-go/consumers] %s bound to %s/%s", c.name, stream, durable)
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
			log.Printf("[cc-go/consumers] %s unsubscribe %s: %v", c.name, sub.Subject, err)
		}
	}
	c.subs = nil
}

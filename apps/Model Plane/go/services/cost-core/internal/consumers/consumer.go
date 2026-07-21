// Package consumers hosts in-process JetStream consumers for cost-core.
//
// DurableConsumer is a thin scaffold over a nats.JetStreamContext for durable
// push consumers bound to a PRE-PROVISIONED server-side consumer (never
// self-created — see BindProvisioned). It owns the subscribe / manual-ack /
// drain boilerplate so the handler only writes its own logic. Modelled on
// conversation-core-go/internal/consumers (Application Plane) and
// notification-core/internal/consumers.
package consumers

import (
	"log"

	"github.com/nats-io/nats.go"
)

// controlSharedStream carries platform-wide, cross-plane events published by
// Control-Plane services (e.g. org-core's GDPR erasure fan-out) that are not
// scoped to any single plane's own event namespace. Mirrors
// conversation-core-go's and documents-api-go's identical constant and
// audit-core's provisioner (ControlSharedStreamName / stream
// "AQENCIA_CONTROLPLANE").
const controlSharedStream = "AQENCIA_CONTROLPLANE"

// DurableConsumer binds one durable push subscription and drains it on
// shutdown. The binding uses manual ack so a handler has time to apply its
// side effect (a database purge) before acking.
type DurableConsumer struct {
	js   nats.JetStreamContext
	name string
	sub  *nats.Subscription
}

func NewDurableConsumer(js nats.JetStreamContext, name string) *DurableConsumer {
	return &DurableConsumer{js: js, name: name}
}

// BindProvisioned attaches to a durable consumer that must already exist
// server-side on stream (created by audit-core's provisioner, never by this
// process). nats.Bind — NOT nats.Durable alone, and NOT a self-provisioning
// call like AddConsumer/QueueSubscribe-without-Bind — is what makes this a
// bind-only attach: a narrowly-scoped NATS identity is granted
// CONSUMER.INFO/MSG.NEXT/ACK on this one durable, never the broad
// CONSUMER.CREATE admin right self-provisioning would require.
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
	c.sub = sub
	log.Printf("[cost-core/consumers] %s bound to %s/%s", c.name, stream, durable)
	return nil
}

// Stop drains the bound subscription.
func (c *DurableConsumer) Stop() {
	if c == nil || c.sub == nil {
		return
	}
	if err := c.sub.Unsubscribe(); err != nil {
		log.Printf("[cost-core/consumers] %s unsubscribe %s: %v", c.name, c.sub.Subject, err)
	}
	c.sub = nil
}

// outcome tells the message handler whether to ack (done, or a definitive
// no-op skip) or retry (transient failure — do not ack so JetStream
// redelivers).
type outcome int

const (
	outcomeAck outcome = iota
	outcomeRetry
)

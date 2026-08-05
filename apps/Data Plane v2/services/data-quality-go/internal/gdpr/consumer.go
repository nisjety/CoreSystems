package gdpr

import (
	"context"
	"errors"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"
)

const (
	// controlSharedStream is the shared cross-plane bus stream owned by
	// audit-core's provisioner
	// (apps/Control Plane/audit-core/internal/provisioner/provisioner.go),
	// which already carries ErasureRequestedSubject via its
	// GDPRErasureRequestedSubject constant. No new stream or subject is
	// needed here — only a new pre-provisioned CONSUMER on it.
	controlSharedStream = "AQENCIA_CONTROLPLANE"

	// OrgErasureDurableName is this service's own pre-provisioned JetStream
	// consumer name on controlSharedStream. It MUST be added to audit-core's
	// provisioner (see the package doc and this file's Start) before Start
	// can bind: Start uses nats.Bind (never nats.Durable/create_consumer), so
	// self-provisioning is not possible with this service's narrowly-scoped
	// shared-broker identity, which must never be granted JetStream
	// CONSUMER.CREATE admin rights.
	OrgErasureDurableName = "data-quality-org-erasure"

	// OrgErasureDeliverySubject is the push-consumer deliver subject
	// audit-core's provisioner must configure on OrgErasureDurableName,
	// mirroring the "_VEREVON.CONTROL.SHARED.DELIVER.<plane>.<service>.<name>"
	// convention already used by documents-api-go/conversation-core-go/
	// quarry-control's own org-erasure consumers.
	OrgErasureDeliverySubject = "_VEREVON.CONTROL.SHARED.DELIVER.data.data-quality.org-erasure"
)

// Consumer binds the pre-provisioned durable consumer and purges
// organizations as their erasure fan-out arrives.
type Consumer struct {
	sub *nats.Subscription
}

// Start binds to OrgErasureDurableName via nats.Bind on nc — a connection to
// the SHARED cross-plane broker (control-shared-nats), never this service's
// plane-local broker (data-quality-go has none of its own; see cmd/main.go).
// The consumer must already be provisioned on controlSharedStream before this
// can bind (ConsumerInfo/subscribe will fail otherwise) — that provisioning
// happens in audit-core's provisioner, not here.
func Start(nc *nats.Conn, repo OrgPurger) (*Consumer, error) {
	if nc == nil {
		return nil, errors.New("GDPR org-erasure consumer requires a NATS connection")
	}
	if repo == nil {
		return nil, errors.New("GDPR org-erasure consumer requires an OrgPurger")
	}
	js, err := nc.JetStream()
	if err != nil {
		return nil, err
	}
	subscription, err := js.QueueSubscribe(ErasureRequestedSubject, OrgErasureDurableName, func(msg *nats.Msg) {
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		handle(ctx, msg, repo)
	}, nats.Bind(controlSharedStream, OrgErasureDurableName), nats.ManualAck())
	if err != nil {
		return nil, err
	}
	log.Info().Str("consumer", OrgErasureDurableName).Str("subject", ErasureRequestedSubject).
		Msg("data-quality-go: durable GDPR org-erasure consumer bound")
	return &Consumer{sub: subscription}, nil
}

// handle decodes and processes one delivery, then acks or NAKs based on the
// outcome: a transient purge failure (e.g. Postgres unreachable) NAKs so
// JetStream redelivers; a poison event (malformed, or non-organization —
// resolved inside HandleOrgErasure) acks immediately since retrying can never
// fix it. Ack only ever follows either a successful purge or a confirmed
// no-op/poison classification, never a bare "give up".
func handle(ctx context.Context, msg *nats.Msg, repo OrgPurger) {
	err := HandleOrgErasure(ctx, repo, msg.Data)
	var poison *poisonOrgEventError
	if err != nil && !errors.As(err, &poison) {
		if nakErr := msg.Nak(); nakErr != nil {
			log.Error().Err(nakErr).Msg("data-quality-go: nak GDPR org-erasure delivery")
		}
		log.Error().Err(err).Msg("data-quality-go: GDPR org-erasure purge deferred, will retry")
		return
	}
	if err != nil {
		log.Error().Err(err).Msg("data-quality-go: GDPR org-erasure event is poison; acking to avoid infinite redelivery")
	}
	if ackErr := msg.Ack(); ackErr != nil {
		log.Error().Err(ackErr).Msg("data-quality-go: ack GDPR org-erasure delivery")
	}
}

// Close drains the subscription.
func (c *Consumer) Close() error {
	if c == nil || c.sub == nil {
		return nil
	}
	return c.sub.Drain()
}

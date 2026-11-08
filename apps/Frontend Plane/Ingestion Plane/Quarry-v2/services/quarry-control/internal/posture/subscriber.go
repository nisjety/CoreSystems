package posture

import (
	"errors"
	"fmt"
	"strconv"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog"
)

// StartSubscriber binds quarry-control's pre-provisioned durable push consumer
// (DurableConsumerName on the CONTROLPLANE stream, filter ZDREnabledSubject)
// and applies every delivery through c. Like gdpr.StartSubscriber it uses
// nats.Bind: quarry-control's shared-broker identity holds no consumer
// create/update authority, so the consumer row must exist in audit-core's
// provisioner and the identity needs receive and ack permission on its
// delivery subject. The caller drains the subscription on shutdown.
func StartSubscriber(nc *nats.Conn, c *Consumer, logger zerolog.Logger) (*nats.Subscription, error) {
	if nc == nil {
		return nil, fmt.Errorf("posture: StartSubscriber requires a NATS connection")
	}
	if c == nil {
		return nil, fmt.Errorf("posture: StartSubscriber requires a Consumer")
	}
	if c.Mode() != ModeObserve && c.Mode() != ModeEnforce {
		return nil, fmt.Errorf("posture: consumer mode %q does not subscribe", c.Mode())
	}
	js, err := nc.JetStream()
	if err != nil {
		return nil, fmt.Errorf("posture: init JetStream context: %w", err)
	}
	sub, err := js.QueueSubscribe(ZDREnabledSubject, DurableConsumerName,
		func(msg *nats.Msg) { handleDelivery(msg, c, logger) },
		nats.Bind(controlSharedStream, DurableConsumerName),
		nats.ManualAck(),
	)
	if err != nil {
		return nil, fmt.Errorf("posture: subscribe %s: %w", ZDREnabledSubject, err)
	}
	logger.Info().
		Str("subject", ZDREnabledSubject).
		Str("durable", DurableConsumerName).
		Str("mode", string(c.Mode())).
		Msg("posture: ZDR-enabled consumer started")
	return sub, nil
}

// handleDelivery acks a processed or poison delivery and naks a failed one
// (store unreachable) with a short delay so JetStream redelivers. Mirrors the
// gdpr subscriber's ack/nak split.
func handleDelivery(msg *nats.Msg, c *Consumer, logger zerolog.Logger) {
	_, err := c.Handle(msg.Data, deliveryKey(msg))
	var poison *poisonEventError
	switch {
	case err != nil && !errors.As(err, &poison):
		logger.Error().Err(err).Str("subject", msg.Subject).Msg("posture: ZDR-enabled event failed - will retry")
		_ = msg.NakWithDelay(time.Second)
		return
	case err != nil:
		logger.Warn().Err(err).Str("subject", msg.Subject).Msg("posture: dropping unprocessable ZDR-enabled event")
	}
	if ackErr := msg.Ack(); ackErr != nil {
		logger.Error().Err(ackErr).Str("subject", msg.Subject).Msg("posture: ack failed")
	}
}

// deliveryKey identifies the transition a delivery carries: org-core's
// Nats-Msg-Id (unique per transition, the same on every redelivery), else the
// stream sequence, else empty.
func deliveryKey(msg *nats.Msg) string {
	if msg == nil {
		return ""
	}
	if msg.Header != nil {
		if id := msg.Header.Get(nats.MsgIdHdr); id != "" {
			return id
		}
	}
	if meta, err := msg.Metadata(); err == nil && meta != nil {
		return "seq:" + strconv.FormatUint(meta.Sequence.Stream, 10)
	}
	return ""
}

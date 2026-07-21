package gdpr

import (
	"errors"
	"fmt"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog"
)

// DurableConsumerName is quarry-control's durable name on the shared
// cross-plane GDPR erasure fan-out — mirrors the "<service>-org-erasure"
// naming convention the rest of this workstream's org-purge consumers use
// (documents-api-go's "documents-api-org-erasure", conversation-core-go's
// "conversation-core-org-erasure"). It also doubles as the queue-group
// name, so N quarry-control replicas process each event once per replica
// set, not once per replica.
const DurableConsumerName = "quarry-control-org-erasure"

// controlSharedStream is the shared cross-plane control-plane JetStream
// stream that carries velion.gdpr.erasure.requested (and other
// Control-Plane-originated lifecycle events). Mirrors the
// controlSharedStream constant documents-api-go's and conversation-core-go's
// org-purge consumers bind against.
const controlSharedStream = "AQENCIA_CONTROLPLANE"

// StartSubscriber binds quarry-control's pre-provisioned durable JetStream
// push consumer (DurableConsumerName, on controlSharedStream) and purges the
// org's crawl data via purger for every "organization" erasure event
// received. The returned subscription should be drained/unsubscribed on
// shutdown.
//
// Like documents-api-go's and conversation-core-go's org-purge consumers,
// this nats.Binds to a consumer that must already be pre-provisioned by
// audit-core's provisioner (apps/Control Plane/audit-core/internal/provisioner)
// against controlSharedStream, rather than self-provisioning via
// nats.Durable(...). quarry-control's shared-broker identity
// ("quarry-control-gdpr") is granted only INFO/ACK/deliver-subject
// permissions on control-shared-nats — no JetStream consumer
// create/update/delete authority — so self-provisioning would fail a
// permissions check before it ever reached "consumer not found". AckWait and
// MaxAckPending are therefore fixed server-side properties of the
// pre-provisioned consumer and are not passed here; it still gets a genuine
// JetStream durable consumer — explicit ack and nak-triggered redelivery —
// the real at-least-once guarantee the fixed contract's idempotency
// requirement is written against.
func StartSubscriber(nc *nats.Conn, purger OrgPurger, logger zerolog.Logger) (*nats.Subscription, error) {
	if nc == nil {
		return nil, fmt.Errorf("gdpr: StartSubscriber requires a NATS connection")
	}
	if purger == nil {
		return nil, fmt.Errorf("gdpr: StartSubscriber requires an OrgPurger")
	}
	js, err := nc.JetStream()
	if err != nil {
		return nil, fmt.Errorf("gdpr: init JetStream context: %w", err)
	}
	sub, err := js.QueueSubscribe(ErasureRequestedSubject, DurableConsumerName,
		func(msg *nats.Msg) { handleDelivery(msg, purger, logger) },
		nats.Bind(controlSharedStream, DurableConsumerName),
		nats.ManualAck(),
	)
	if err != nil {
		return nil, fmt.Errorf("gdpr: subscribe %s: %w", ErasureRequestedSubject, err)
	}
	logger.Info().
		Str("subject", ErasureRequestedSubject).
		Str("durable", DurableConsumerName).
		Msg("gdpr: org-erasure purge consumer started")
	return sub, nil
}

// handleDelivery applies HandleErasure to one NATS delivery and resolves it
// to an ack or a nak:
//   - a poison payload (malformed JSON) acks immediately — no amount of
//     redelivery turns bad input into good input;
//   - a legitimate no-op (wrong subject_type, missing org_id) also acks —
//     it is a definitive, deliberate skip, not a failure;
//   - any other error (the store is unreachable, a transient DB failure)
//     naks with a short delay so JetStream redelivers.
//
// This mirrors the ack/nak split the sibling org-purge consumers
// (documents-api-go, conversation-core-go) use for the same event.
func handleDelivery(msg *nats.Msg, purger OrgPurger, logger zerolog.Logger) {
	result, err := HandleErasure(purger, msg.Data)
	var poison *poisonEventError
	if err != nil && !errors.As(err, &poison) {
		logger.Error().Err(err).Str("subject", msg.Subject).Msg("gdpr: erasure purge failed — will retry")
		_ = msg.NakWithDelay(time.Second)
		return
	}
	switch {
	case err != nil:
		logger.Warn().Err(err).Str("subject", msg.Subject).Msg("gdpr: dropping unprocessable erasure event")
	case result.Total() > 0:
		logger.Info().
			Int64("jobs_deleted", result.JobsDeleted).
			Int64("schedules_deleted", result.SchedulesDeleted).
			Int64("sources_deleted", result.SourcesDeleted).
			Int64("benchmarks_deleted", result.BenchmarksDeleted).
			Int64("idempotency_keys_deleted", result.IdempotencyKeysDeleted).
			Msg("gdpr: org-scoped crawl data purged")
	}
	if ackErr := msg.Ack(); ackErr != nil {
		logger.Error().Err(ackErr).Str("subject", msg.Subject).Msg("gdpr: ack failed")
	}
}

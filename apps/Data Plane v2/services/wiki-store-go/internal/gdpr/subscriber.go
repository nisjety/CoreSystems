package gdpr

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"
)

// DurableConsumerName is wiki-store-go's durable name on the shared
// cross-plane GDPR erasure fan-out — mirrors the "<service>-org-erasure"
// naming convention the rest of this workstream's org-purge consumers use
// (documents-api-go's "documents-api-org-erasure", conversation-core-go's
// "conversation-core-org-erasure", quarry-control's
// "quarry-control-org-erasure"). It also doubles as the queue-group name, so
// N wiki-store-go replicas process each event once per replica set, not once
// per replica.
const DurableConsumerName = "wiki-store-org-erasure"

// controlSharedStream is the shared cross-plane JetStream stream that
// carries velion.gdpr.erasure.requested (and other Control-Plane-originated
// lifecycle events). Mirrors the controlSharedStream constant
// documents-api-go's, conversation-core-go's, and quarry-control's org-purge
// consumers bind against. Provisioned by
// apps/Control Plane/audit-core/internal/provisioner.
const controlSharedStream = "AQENCIA_CONTROLPLANE"

// StartSubscriber binds wiki-store-go's pre-provisioned durable JetStream
// push consumer (DurableConsumerName, on controlSharedStream) and purges an
// org's wiki + Operating Map data via purger for every "organization"
// erasure event received. The returned subscription should be
// drained/unsubscribed on shutdown.
//
// Like documents-api-go's, conversation-core-go's, and quarry-control's
// org-purge consumers, this nats.Binds to a consumer that must already be
// pre-provisioned by audit-core's provisioner against controlSharedStream,
// rather than self-provisioning via nats.Durable(...) alone. wiki-store-go's
// shared-broker identity ("wiki-store-gdpr") is granted only
// CONSUMER.INFO/ACK/deliver-subject permissions on control-shared-nats — no
// JetStream consumer create/update/delete authority — so self-provisioning
// would fail a permissions check before it ever reached "consumer not
// found". This client library (nats.go) resolves a durable push
// subscription via CONSUMER.INFO + the consumer's own deliver subject; it
// never calls STREAM.INFO, so wiki-store-gdpr's control-shared-nats grant
// does not need $JS.API.STREAM.INFO.AQENCIA_CONTROLPLANE (that permission is
// only required for client libraries whose Context/JetStream initialization
// path calls get_stream/GetStream before resolving the consumer — see
// Model Plane session-core's async-nats 0.49.1 consumer for that case).
func StartSubscriber(nc *nats.Conn, purger OrgPurger) (*nats.Subscription, error) {
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
		func(msg *nats.Msg) { handleDelivery(msg, purger) },
		nats.Bind(controlSharedStream, DurableConsumerName),
		nats.ManualAck(),
	)
	if err != nil {
		return nil, fmt.Errorf("gdpr: subscribe %s: %w", ErasureRequestedSubject, err)
	}
	log.Info().
		Str("subject", ErasureRequestedSubject).
		Str("durable", DurableConsumerName).
		Msg("gdpr: wiki org-erasure purge consumer started")
	return sub, nil
}

// handleDelivery applies HandleOrgErasure to one NATS delivery and resolves
// it to an ack or a nak:
//   - a poison payload (malformed JSON, or an "organization" event missing
//     its scope) acks immediately — no amount of redelivery turns bad input
//     into good input;
//   - a legitimate no-op (wrong subject_type) also acks — it is a
//     definitive, deliberate skip, not a failure;
//   - any other error (Postgres unreachable, a transient failure) naks with
//     a short delay so JetStream redelivers.
//
// This mirrors the ack/nak split the sibling org-purge consumers
// (documents-api-go, conversation-core-go, quarry-control) use for the same
// event.
func handleDelivery(msg *nats.Msg, purger OrgPurger) {
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()

	result, err := HandleOrgErasure(ctx, purger, msg.Data)
	var poison *poisonEventError
	if err != nil && !errors.As(err, &poison) {
		log.Error().Err(err).Str("subject", msg.Subject).Msg("gdpr: wiki org-erasure purge failed — will retry")
		_ = msg.NakWithDelay(time.Second)
		return
	}
	switch {
	case err != nil:
		log.Warn().Err(err).Str("subject", msg.Subject).Msg("gdpr: dropping unprocessable erasure event")
	case result.Total() > 0:
		log.Info().
			Int64("wiki_pages_deleted", result.WikiPagesDeleted).
			Int64("wiki_source_logs_deleted", result.WikiSourceLogsDeleted).
			Int64("wiki_maintenance_logs_deleted", result.WikiMaintenanceLogsDeleted).
			Int64("operating_maps_deleted", result.OperatingMapsDeleted).
			Int64("wiki_event_outbox_deleted", result.WikiEventOutboxDeleted).
			Msg("gdpr: org-scoped wiki + operating map data purged")
	}
	if ackErr := msg.Ack(); ackErr != nil {
		log.Error().Err(ackErr).Str("subject", msg.Subject).Msg("gdpr: ack failed")
	}
}

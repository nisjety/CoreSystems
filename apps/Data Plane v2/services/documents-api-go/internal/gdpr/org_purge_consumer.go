package gdpr

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"
)

// The org-purge durable is a SEPARATE JetStream consumer from
// durableConsumerName (subscriber.go's per-user ownership-transfer durable),
// even though both filter on the same ErasureRequestedSubject: they own
// disjoint subject_type values from the same fan-out, are independently
// scaled/retried/DLQ'd, and a failure in one must never stall or poison the
// other. Like durableConsumerName, this durable is pre-provisioned (not
// self-provisioned — this service's shared-broker identity has no consumer
// administration rights, matching the "runtime services never receive
// stream/consumer create/update/delete authority" rule enforced by
// apps/Control Plane/audit-core/internal/provisioner/provisioner.go) and this
// code only binds to it.
const (
	orgPurgeDurableConsumerName    = "documents-api-org-erasure"
	orgPurgeDurableDeliverySubject = "_VELION.CONTROL.SHARED.DELIVER.data.documents-api.org-erasure"
	orgPurgeDLQSubject             = "velion.gdpr.erasure.dlq.documents-api-org-purge"
)

// publishOrgPurgeDLQ mirrors subscriber.go's publishDLQ, addressed to this
// consumer's own DLQ subject/name so a terminal org-purge failure is never
// conflated with a terminal per-user ownership-transfer failure in the same
// DLQ stream.
func publishOrgPurgeDLQ(ctx context.Context, publisher dlqPublisher, payload []byte, attempts uint64, cause error) error {
	digest := sha256.Sum256(payload)
	body, err := json.Marshal(map[string]any{
		"event_id":          fmt.Sprintf("gdpr:documents-api-org-purge:dlq:%x", digest),
		"failed_subject":    ErasureRequestedSubject,
		"consumer":          orgPurgeDurableConsumerName,
		"delivery_attempts": attempts,
		"error":             truncateError(cause.Error(), 1000),
		"payload_base64":    payload,
		"failed_at":         time.Now().UTC().Format(time.RFC3339Nano),
	})
	if err != nil {
		return fmt.Errorf("encode org purge DLQ evidence: %w", err)
	}
	message := nats.NewMsg(orgPurgeDLQSubject)
	message.Header.Set(nats.MsgIdHdr, fmt.Sprintf("gdpr:documents-api-org-purge:dlq:%x", digest))
	message.Data = body
	ack, err := publisher.PublishMsg(message, nats.Context(ctx))
	if err != nil {
		return fmt.Errorf("publish org purge DLQ: %w", err)
	}
	if ack == nil || ack.Stream != controlSharedStream || ack.Sequence == 0 {
		return fmt.Errorf("publish org purge DLQ: invalid JetStream PubAck")
	}
	return nil
}

// processOrgPurgeDelivery mirrors subscriber.go's processDelivery: NAK on a
// transient failure (Postgres unreachable, etc.) so JetStream redelivers;
// route poison (malformed event, or an "organization" event missing scope)
// straight to the DLQ and ACK immediately since no amount of retrying fixes
// bad input; ACK only after either a successful purge or a confirmed DLQ
// write, never before.
func processOrgPurgeDelivery(ctx context.Context, delivery durableDelivery, repo OrgPurger, dlq dlqPublisher, health *consumerHealth) error {
	attempts, err := delivery.DeliveryAttempts()
	if err != nil {
		health.recordError(err)
		_ = delivery.NakWithDelay(time.Second)
		return fmt.Errorf("read org purge delivery metadata: %w", err)
	}
	handleErr := HandleOrgErasure(ctx, repo, delivery.Payload())
	var poison *poisonOrgEventError
	terminal := errors.As(handleErr, &poison) || (handleErr != nil && attempts >= terminalDeliveryAttempt)
	if handleErr != nil && !terminal {
		health.recordError(handleErr)
		_ = delivery.NakWithDelay(time.Duration(attempts) * time.Second)
		return handleErr
	}
	if terminal {
		if dlqErr := publishOrgPurgeDLQ(ctx, dlq, delivery.Payload(), attempts, handleErr); dlqErr != nil {
			health.recordError(dlqErr)
			_ = delivery.NakWithDelay(time.Second)
			return dlqErr
		}
		health.recordTerminal()
	}
	if err := delivery.Ack(); err != nil {
		health.recordError(err)
		return fmt.Errorf("ACK org purge delivery: %w", err)
	}
	health.recordSuccess()
	return nil
}

// StartOrgPurgeSubscriber binds the pre-provisioned "documents-api-org-erasure"
// durable and starts purging organizations as their erasure fan-out arrives.
// Returns the same *Consumer type subscriber.go's StartSubscriber returns —
// Close()/Health() behave identically, just against this durable's own
// JetStream ConsumerInfo.
func StartOrgPurgeSubscriber(nc *nats.Conn, repo OrgPurger) (*Consumer, error) {
	if nc == nil || repo == nil {
		return nil, fmt.Errorf("org-purge durable consumer requires NATS and repository")
	}
	js, err := nc.JetStream()
	if err != nil {
		return nil, fmt.Errorf("initialize org-purge JetStream consumer: %w", err)
	}
	info, err := js.ConsumerInfo(controlSharedStream, orgPurgeDurableConsumerName)
	if err != nil {
		return nil, fmt.Errorf("bind pre-provisioned org-purge consumer: %w", err)
	}
	config := info.Config
	if config.FilterSubject != ErasureRequestedSubject || config.DeliverSubject != orgPurgeDurableDeliverySubject ||
		config.Durable != orgPurgeDurableConsumerName || config.AckPolicy != nats.AckExplicitPolicy || config.MaxDeliver != consumerMaxDeliver {
		return nil, fmt.Errorf("pre-provisioned org-purge consumer has incompatible configuration")
	}
	health := newConsumerHealth()
	subscription, err := js.QueueSubscribe(ErasureRequestedSubject, orgPurgeDurableConsumerName, func(message *nats.Msg) {
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		if err := processOrgPurgeDelivery(ctx, natsDelivery{message: message}, repo, js, health); err != nil {
			log.Error().Err(err).Msg("durable org-purge deferred")
		}
	}, nats.Bind(controlSharedStream, orgPurgeDurableConsumerName), nats.ManualAck())
	if err != nil {
		return nil, fmt.Errorf("subscribe durable org-purge consumer: %w", err)
	}
	log.Info().Str("consumer", orgPurgeDurableConsumerName).Str("subject", ErasureRequestedSubject).Msg("durable org-purge consumer started")
	return &Consumer{js: js, sub: subscription, health: health, name: orgPurgeDurableConsumerName}, nil
}

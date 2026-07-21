package gdpr

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"sync"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"
)

// This service's shared-broker identity ("data-orchestrator-gdpr") has no
// stream or consumer administration rights — it can only bind to a
// pre-provisioned durable via nats.Bind, never self-provision one via
// nats.Durable/AddConsumer. That is enforced by
// apps/Control Plane/audit-core/internal/provisioner/provisioner.go, which
// is the only code path allowed to create/update/delete JetStream
// consumers on the shared broker.
const (
	controlSharedStream     = "AQENCIA_CONTROLPLANE"
	durableConsumerName     = "data-orchestrator-org-erasure"
	durableDeliverySubject  = "_VELION.CONTROL.SHARED.DELIVER.data.data-orchestrator.org-erasure"
	erasureDLQSubject       = "velion.gdpr.erasure.dlq.data-orchestrator-org-purge"
	terminalDeliveryAttempt = uint64(10)
	consumerMaxDeliver      = 20
)

type durableDelivery interface {
	Payload() []byte
	DeliveryAttempts() (uint64, error)
	Ack() error
	NakWithDelay(time.Duration) error
}

type dlqPublisher interface {
	PublishMsg(*nats.Msg, ...nats.PubOpt) (*nats.PubAck, error)
}

type natsDelivery struct{ message *nats.Msg }

func (d natsDelivery) Payload() []byte { return d.message.Data }
func (d natsDelivery) DeliveryAttempts() (uint64, error) {
	metadata, err := d.message.Metadata()
	if err != nil {
		return 0, err
	}
	return metadata.NumDelivered, nil
}
func (d natsDelivery) Ack() error                             { return d.message.Ack() }
func (d natsDelivery) NakWithDelay(delay time.Duration) error { return d.message.NakWithDelay(delay) }

type ConsumerHealthSnapshot struct {
	Status          string `json:"status"`
	Pending         uint64 `json:"pending"`
	AckPending      int    `json:"ack_pending"`
	Redelivered     int    `json:"redelivered"`
	TerminalCount   uint64 `json:"terminal_count"`
	Acknowledged    uint64 `json:"acknowledged"`
	LastSuccessAt   string `json:"last_success_at,omitempty"`
	LastError       string `json:"last_error,omitempty"`
	LastActivityLag int64  `json:"last_activity_lag_ms"`
}

func (snapshot ConsumerHealthSnapshot) NumOutstanding() uint64 {
	return snapshot.Pending + uint64(max(snapshot.AckPending, 0))
}

type consumerHealth struct {
	mu            sync.RWMutex
	terminalCount uint64
	acknowledged  uint64
	lastSuccess   time.Time
	lastError     string
}

func newConsumerHealth() *consumerHealth { return &consumerHealth{} }

func (health *consumerHealth) recordSuccess() {
	health.mu.Lock()
	defer health.mu.Unlock()
	health.acknowledged++
	health.lastSuccess = time.Now().UTC()
	health.lastError = ""
}

func (health *consumerHealth) recordError(err error) {
	health.mu.Lock()
	defer health.mu.Unlock()
	health.lastError = err.Error()
}

func (health *consumerHealth) recordTerminal() {
	health.mu.Lock()
	defer health.mu.Unlock()
	health.terminalCount++
}

func truncateError(value string, limit int) string {
	if len(value) <= limit {
		return value
	}
	return value[:limit]
}

// publishDLQ mirrors the sibling GDPR consumers in this program (e.g.
// documents-api-go's internal/gdpr) so a terminal org-purge failure always
// leaves reconstructable evidence instead of being silently dropped.
func publishDLQ(ctx context.Context, publisher dlqPublisher, payload []byte, attempts uint64, cause error) error {
	digest := sha256.Sum256(payload)
	body, err := json.Marshal(map[string]any{
		"event_id":          fmt.Sprintf("gdpr:data-orchestrator-org-purge:dlq:%x", digest),
		"failed_subject":    ErasureRequestedSubject,
		"consumer":          durableConsumerName,
		"delivery_attempts": attempts,
		"error":             truncateError(cause.Error(), 1000),
		// []byte is encoded as base64, so malformed JSON remains
		// reconstructable instead of making the DLQ envelope itself
		// impossible to encode.
		"payload_base64": payload,
		"failed_at":      time.Now().UTC().Format(time.RFC3339Nano),
	})
	if err != nil {
		return fmt.Errorf("encode org purge DLQ evidence: %w", err)
	}
	message := nats.NewMsg(erasureDLQSubject)
	message.Header.Set(nats.MsgIdHdr, fmt.Sprintf("gdpr:data-orchestrator-org-purge:dlq:%x", digest))
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

// processOrgPurgeDelivery NAKs on a transient failure (Postgres unreachable,
// etc.) so JetStream redelivers; routes poison (malformed event, or an
// "organization" event missing scope) straight to the DLQ and ACKs
// immediately since no amount of retrying fixes bad input; ACKs only after
// either a successful purge or a confirmed DLQ write, never before.
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
		if dlqErr := publishDLQ(ctx, dlq, delivery.Payload(), attempts, handleErr); dlqErr != nil {
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

type Consumer struct {
	js     nats.JetStreamContext
	sub    *nats.Subscription
	health *consumerHealth
}

// StartOrgPurgeSubscriber binds the pre-provisioned
// "data-orchestrator-org-erasure" durable and starts purging organizations
// as their erasure fan-out arrives. It never self-provisions the
// consumer — nc's identity is scoped to CONSUMER.INFO/ACK on this one
// durable and has no CONSUMER.CREATE (stream/consumer administration)
// rights, matching the shared-broker authority model every GDPR consumer in
// this program follows.
func StartOrgPurgeSubscriber(nc *nats.Conn, repo OrgPurger) (*Consumer, error) {
	if nc == nil || repo == nil {
		return nil, fmt.Errorf("org-purge durable consumer requires NATS and repository")
	}
	js, err := nc.JetStream()
	if err != nil {
		return nil, fmt.Errorf("initialize org-purge JetStream consumer: %w", err)
	}
	info, err := js.ConsumerInfo(controlSharedStream, durableConsumerName)
	if err != nil {
		return nil, fmt.Errorf("bind pre-provisioned org-purge consumer: %w", err)
	}
	config := info.Config
	if config.FilterSubject != ErasureRequestedSubject || config.DeliverSubject != durableDeliverySubject ||
		config.Durable != durableConsumerName || config.AckPolicy != nats.AckExplicitPolicy || config.MaxDeliver != consumerMaxDeliver {
		return nil, fmt.Errorf("pre-provisioned org-purge consumer has incompatible configuration")
	}
	health := newConsumerHealth()
	subscription, err := js.QueueSubscribe(ErasureRequestedSubject, durableConsumerName, func(message *nats.Msg) {
		ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
		defer cancel()
		if err := processOrgPurgeDelivery(ctx, natsDelivery{message: message}, repo, js, health); err != nil {
			log.Error().Err(err).Msg("durable org-purge deferred")
		}
	}, nats.Bind(controlSharedStream, durableConsumerName), nats.ManualAck())
	if err != nil {
		return nil, fmt.Errorf("subscribe durable org-purge consumer: %w", err)
	}
	log.Info().Str("consumer", durableConsumerName).Str("subject", ErasureRequestedSubject).Msg("durable org-purge consumer started")
	return &Consumer{js: js, sub: subscription, health: health}, nil
}

func (consumer *Consumer) Close() error {
	if consumer == nil || consumer.sub == nil {
		return nil
	}
	return consumer.sub.Drain()
}

func (consumer *Consumer) Health(ctx context.Context) ConsumerHealthSnapshot {
	if consumer == nil || consumer.js == nil || consumer.health == nil {
		return ConsumerHealthSnapshot{Status: "unhealthy", LastError: "consumer unavailable"}
	}
	consumer.health.mu.RLock()
	snapshot := ConsumerHealthSnapshot{
		Status: "healthy", TerminalCount: consumer.health.terminalCount,
		Acknowledged: consumer.health.acknowledged, LastError: consumer.health.lastError,
	}
	lastSuccess := consumer.health.lastSuccess
	consumer.health.mu.RUnlock()
	if !lastSuccess.IsZero() {
		snapshot.LastSuccessAt = lastSuccess.Format(time.RFC3339Nano)
		snapshot.LastActivityLag = time.Since(lastSuccess).Milliseconds()
	}
	info, err := consumer.js.ConsumerInfo(controlSharedStream, durableConsumerName, nats.Context(ctx))
	if err != nil {
		snapshot.Status = "degraded"
		snapshot.LastError = truncateError(err.Error(), 1000)
		return snapshot
	}
	snapshot.Pending = info.NumPending
	snapshot.AckPending = info.NumAckPending
	snapshot.Redelivered = info.NumRedelivered
	if snapshot.NumOutstanding() > 0 && lastSuccess.IsZero() {
		snapshot.LastActivityLag = time.Since(info.Created).Milliseconds()
	}
	if snapshot.TerminalCount > 0 || snapshot.LastError != "" || snapshot.NumOutstanding() > 0 && snapshot.LastActivityLag > int64((5*time.Minute).Milliseconds()) {
		snapshot.Status = "degraded"
	}
	return snapshot
}

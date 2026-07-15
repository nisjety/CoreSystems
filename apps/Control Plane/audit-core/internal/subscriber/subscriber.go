// Package subscriber wires NATS subscriptions for the two event
// subject hierarchies that audit-core persists:
//
//   - `velion.audit.v2.<plane>.<producer>.<event>` — security/operational events.
//   - `velion.usage.v2.<plane>.<producer>.<op>`    — billable resource usage.
//
// Both are durable JetStream consumers. Successful database writes are ACKed,
// transient store failures are NAKed for bounded redelivery, and malformed or
// exhausted messages are terminated after a durable dead-letter copy is made.
package subscriber

import (
	"context"
	"fmt"
	"strings"
	"sync/atomic"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/events"
	metricsserver "github.com/triodelab/controlplane/audit-core/internal/metrics"
)

const (
	auditSubject          = "velion.audit.v2.>"
	usageSubject          = "velion.usage.v2.>"
	dlqSubject            = "velion.dlq.audit-core.>"
	streamName            = "VELION_CONTROL_OBSERVABILITY"
	maxDeliveries         = 5
	maxConsumerDeliveries = maxDeliveries
	consumerHealthMaxWait = 250 * time.Millisecond
)

type Subscriber struct {
	nc                   *nats.Conn
	js                   nats.JetStreamContext
	store                eventStore
	bus                  string
	plane                string
	auditLastAckUnixNano atomic.Int64
	usageLastAckUnixNano atomic.Int64
}

type ConsumerHealth struct {
	Consumer    string    `json:"consumer"`
	Ready       bool      `json:"ready"`
	Pending     uint64    `json:"pending"`
	AckPending  int       `json:"ack_pending"`
	Redelivered int       `json:"redelivered"`
	LastAckAt   time.Time `json:"last_ack_at,omitempty"`
}

type Health struct {
	Bus   string         `json:"bus"`
	Audit ConsumerHealth `json:"audit"`
	Usage ConsumerHealth `json:"usage"`
}

type eventStore interface {
	InsertAuditFromStream(context.Context, *events.AuditEvent, string, string, uint64) (bool, error)
	InsertUsageFromStream(context.Context, *events.UsageEvent, string, string, uint64) (bool, error)
}

func New(nc *nats.Conn, s eventStore, bus ...string) *Subscriber {
	label := "control"
	plane := "control"
	if len(bus) > 0 && bus[0] != "" {
		label = bus[0]
		plane = bus[0]
	}
	if len(bus) > 1 && bus[1] != "" {
		plane = bus[1]
	}
	return &Subscriber{nc: nc, store: s, bus: label, plane: plane}
}

// Start binds pre-provisioned durable queue consumers. Stream and consumer
// administration belongs to the deployment-only provisioner; the runtime
// principal intentionally cannot create, update, delete, or purge them.
func (s *Subscriber) Start(ctx context.Context) error {
	js, err := s.nc.JetStream()
	if err != nil {
		return fmt.Errorf("open JetStream context: %w", err)
	}
	s.js = js

	auditConsumer := s.consumerName("audit")
	if _, err := js.QueueSubscribe(
		planeSubject("audit", s.plane),
		auditConsumer,
		s.handleAudit(ctx),
		nats.Bind(streamName, auditConsumer),
		nats.ManualAck(),
		nats.AckExplicit(),
		nats.AckWait(30*time.Second),
		nats.MaxDeliver(maxConsumerDeliveries),
		nats.DeliverAll(),
	); err != nil {
		return err
	}
	usageConsumer := s.consumerName("usage")
	if _, err := js.QueueSubscribe(
		planeSubject("usage", s.plane),
		usageConsumer,
		s.handleUsage(ctx),
		nats.Bind(streamName, usageConsumer),
		nats.ManualAck(),
		nats.AckExplicit(),
		nats.AckWait(30*time.Second),
		nats.MaxDeliver(maxConsumerDeliveries),
		nats.DeliverAll(),
	); err != nil {
		return err
	}
	log.Info().
		Str("audit_subject", planeSubject("audit", s.plane)).
		Str("usage_subject", planeSubject("usage", s.plane)).
		Str("stream", streamName).
		Str("audit_consumer", auditConsumer).
		Str("usage_consumer", usageConsumer).
		Msg("audit-core durable subscribers ready")
	return nil
}

func (s *Subscriber) consumerName(kind string) string {
	return fmt.Sprintf("audit-core-%s-v3-%s", s.bus, kind)
}

func planeSubject(kind, plane string) string {
	return fmt.Sprintf("velion.%s.v2.%s.>", strings.TrimSpace(kind), strings.TrimSpace(plane))
}

// Health reports stable durable identities and the JetStream backlog state.
// A missing consumer is not treated as healthy; readiness callers can fail
// closed while the connection supervisor retries subscriber creation.
func (s *Subscriber) Health() Health {
	return Health{
		Bus:   s.bus,
		Audit: s.consumerHealth("audit"),
		Usage: s.consumerHealth("usage"),
	}
}

func (s *Subscriber) consumerHealth(kind string) ConsumerHealth {
	consumer := s.consumerName(kind)
	result := ConsumerHealth{Consumer: consumer, LastAckAt: s.lastAckAt(kind)}
	if s.js == nil {
		return result
	}
	info, err := s.js.ConsumerInfo(
		streamName,
		consumer,
		nats.MaxWait(consumerHealthMaxWait),
	)
	if err != nil {
		return result
	}
	result.Ready = true
	result.Pending = info.NumPending
	result.AckPending = info.NumAckPending
	result.Redelivered = info.NumRedelivered
	metricsserver.SetConsumerState(s.bus, kind, result.Pending, result.AckPending, result.Redelivered)
	return result
}

func (s *Subscriber) handleAudit(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		ev, err := events.DecodeAudit(msg.Data)
		if err != nil {
			metricsserver.RecordEvent(s.bus, "audit", "malformed", time.Time{})
			log.Warn().Err(err).Str("subject", msg.Subject).Msg("audit: drop malformed event")
			if !s.deadLetter(msg, "audit", "malformed") {
				s.nak(msg)
				return
			}
			if err := msg.Term(); err != nil {
				log.Error().Err(err).Str("subject", msg.Subject).Msg("audit: terminate malformed event")
			}
			return
		}
		if !eventAuthorityMatches("audit", msg.Subject, ev.Plane, ev.Producer, ev.Event, s.plane) {
			s.rejectPlaneMismatch(msg, "audit", ev.OccurredAt)
			return
		}
		streamSequence, ok := messageStreamSequence(msg)
		if !ok {
			metricsserver.RecordEvent(s.bus, "audit", "metadata_error", ev.OccurredAt)
			s.nak(msg)
			return
		}
		inserted, err := s.store.InsertAuditFromStream(ctx, ev, s.bus, msg.Subject, streamSequence)
		if err != nil {
			metricsserver.RecordEvent(s.bus, "audit", "store_error", ev.OccurredAt)
			log.Error().Err(err).Str("subject", msg.Subject).Str("org_id", ev.OrgID).
				Msg("audit: insert failed")
			s.retryOrDeadLetter(msg, "audit")
			return
		}
		if !inserted {
			metricsserver.RecordEvent(s.bus, "audit", "duplicate", ev.OccurredAt)
		} else {
			metricsserver.RecordEvent(s.bus, "audit", "persisted", ev.OccurredAt)
		}
		if err := msg.Ack(); err != nil {
			log.Error().Err(err).Str("subject", msg.Subject).Msg("audit: ack failed")
		} else {
			s.recordAck("audit")
		}
	}
}

func (s *Subscriber) handleUsage(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		ev, err := events.DecodeUsage(msg.Data)
		if err != nil {
			metricsserver.RecordEvent(s.bus, "usage", "malformed", time.Time{})
			log.Warn().Err(err).Str("subject", msg.Subject).Msg("usage: drop malformed event")
			if !s.deadLetter(msg, "usage", "malformed") {
				s.nak(msg)
				return
			}
			if err := msg.Term(); err != nil {
				log.Error().Err(err).Str("subject", msg.Subject).Msg("usage: terminate malformed event")
			}
			return
		}
		if !eventAuthorityMatches("usage", msg.Subject, ev.Plane, ev.Producer, ev.Op, s.plane) {
			s.rejectPlaneMismatch(msg, "usage", ev.OccurredAt)
			return
		}
		streamSequence, ok := messageStreamSequence(msg)
		if !ok {
			metricsserver.RecordEvent(s.bus, "usage", "metadata_error", ev.OccurredAt)
			s.nak(msg)
			return
		}
		inserted, err := s.store.InsertUsageFromStream(ctx, ev, s.bus, msg.Subject, streamSequence)
		if err != nil {
			metricsserver.RecordEvent(s.bus, "usage", "store_error", ev.OccurredAt)
			log.Error().Err(err).Str("subject", msg.Subject).Str("org_id", ev.OrgID).
				Msg("usage: insert failed")
			s.retryOrDeadLetter(msg, "usage")
			return
		}
		if !inserted {
			metricsserver.RecordEvent(s.bus, "usage", "duplicate", ev.OccurredAt)
		} else {
			metricsserver.RecordEvent(s.bus, "usage", "persisted", ev.OccurredAt)
		}
		if err := msg.Ack(); err != nil {
			log.Error().Err(err).Str("subject", msg.Subject).Msg("usage: ack failed")
		} else {
			s.recordAck("usage")
		}
	}
}

func (s *Subscriber) recordAck(kind string) {
	now := time.Now().UTC()
	switch kind {
	case "audit":
		s.auditLastAckUnixNano.Store(now.UnixNano())
	case "usage":
		s.usageLastAckUnixNano.Store(now.UnixNano())
	default:
		return
	}
	metricsserver.RecordAck(s.bus, kind, now)
}

func (s *Subscriber) lastAckAt(kind string) time.Time {
	var unixNano int64
	switch kind {
	case "audit":
		unixNano = s.auditLastAckUnixNano.Load()
	case "usage":
		unixNano = s.usageLastAckUnixNano.Load()
	}
	if unixNano == 0 {
		return time.Time{}
	}
	return time.Unix(0, unixNano).UTC()
}

func eventAuthorityMatches(kind, subject, payloadPlane, producer, event, authorityPlane string) bool {
	parts := strings.Split(subject, ".")
	return len(parts) == 6 &&
		parts[0] == "velion" && parts[1] == kind && parts[2] == "v2" &&
		parts[3] == strings.TrimSpace(payloadPlane) &&
		parts[3] == strings.TrimSpace(authorityPlane) &&
		parts[4] == strings.TrimSpace(producer) &&
		parts[5] == strings.TrimSpace(event)
}

func (s *Subscriber) rejectPlaneMismatch(msg *nats.Msg, kind string, occurredAt time.Time) {
	metricsserver.RecordEvent(s.bus, kind, "authority_mismatch", occurredAt)
	log.Warn().Str("subject", msg.Subject).Str("bus", s.bus).Msg("event authority mismatch")
	if !s.deadLetter(msg, kind, "authority_mismatch") {
		s.nak(msg)
		return
	}
	if err := msg.Term(); err != nil {
		log.Error().Err(err).Str("subject", msg.Subject).Msg("terminate plane-mismatched event")
	}
}

func (s *Subscriber) retryOrDeadLetter(msg *nats.Msg, kind string) {
	metadata, err := msg.Metadata()
	if err == nil && metadata.NumDelivered >= maxDeliveries {
		if !s.deadLetter(msg, kind, "delivery_exhausted") {
			s.nak(msg)
			return
		}
		if err := msg.Term(); err != nil {
			log.Error().Err(err).Str("subject", msg.Subject).Msg("terminate exhausted event")
		}
		return
	}
	s.nak(msg)
}

func (s *Subscriber) nak(msg *nats.Msg) {
	if err := msg.NakWithDelay(time.Second); err != nil {
		log.Error().Err(err).Str("subject", msg.Subject).Msg("nak failed")
	}
}

func (s *Subscriber) deadLetter(msg *nats.Msg, kind, reason string) bool {
	streamSequence, ok := messageStreamSequence(msg)
	if !ok {
		log.Error().Str("subject", msg.Subject).Str("reason", reason).
			Msg("dead-letter source metadata unavailable")
		return false
	}
	dlq := nats.NewMsg("velion.dlq.audit-core." + kind)
	dlq.Data = append([]byte(nil), msg.Data...)
	dlq.Header.Set("Velion-Original-Subject", msg.Subject)
	dlq.Header.Set("Velion-Dead-Letter-Reason", reason)
	dlq.Header.Set(nats.MsgIdHdr, deadLetterMessageID(s.bus, kind, reason, streamSequence))
	if _, err := s.js.PublishMsg(dlq); err != nil {
		log.Error().Err(err).Str("subject", msg.Subject).Str("reason", reason).
			Msg("dead-letter publish failed")
		return false
	}
	metricsserver.RecordEvent(s.bus, kind, "dead_lettered", time.Time{})
	return true
}

func deadLetterMessageID(bus, kind, reason string, streamSequence uint64) string {
	return fmt.Sprintf("audit-core-dlq:%s:%s:%s:%d", bus, kind, reason, streamSequence)
}

func messageStreamSequence(msg *nats.Msg) (uint64, bool) {
	metadata, err := msg.Metadata()
	if err != nil || metadata.Sequence.Stream == 0 {
		return 0, false
	}
	return metadata.Sequence.Stream, true
}

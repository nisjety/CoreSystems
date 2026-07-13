// Package subscriber wires NATS subscriptions for the two event
// subject hierarchies that audit-core persists:
//
//   - `velion.audit.v1.<plane>.<event>` — security/operational events.
//   - `velion.usage.v1.<plane>.<op>`    — billable resource usage.
//
// Both are durable JetStream consumers. Successful database writes are ACKed,
// transient store failures are NAKed for bounded redelivery, and malformed or
// exhausted messages are terminated after a durable dead-letter copy is made.
package subscriber

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/events"
	metricsserver "github.com/triodelab/controlplane/audit-core/internal/metrics"
)

const (
	auditSubject          = "velion.audit.v1.>"
	usageSubject          = "velion.usage.v1.>"
	dlqSubject            = "velion.dlq.audit-core.>"
	streamName            = "VELION_CONTROL_OBSERVABILITY"
	maxDeliveries         = 5
	maxConsumerDeliveries = maxDeliveries
)

type Subscriber struct {
	nc    *nats.Conn
	js    nats.JetStreamContext
	store eventStore
	bus   string
}

type eventStore interface {
	InsertAuditFromStream(context.Context, *events.AuditEvent, string, uint64) (bool, error)
	InsertUsageFromStream(context.Context, *events.UsageEvent, string, uint64) (bool, error)
}

func New(nc *nats.Conn, s eventStore, bus ...string) *Subscriber {
	label := "primary"
	if len(bus) > 0 && bus[0] != "" {
		label = bus[0]
	}
	return &Subscriber{nc: nc, store: s, bus: label}
}

// Start creates or updates the owned stream and binds two durable queue
// consumers. Caller is responsible for draining the underlying connection.
func (s *Subscriber) Start(ctx context.Context) error {
	js, err := s.nc.JetStream()
	if err != nil {
		return fmt.Errorf("open JetStream context: %w", err)
	}
	if err := ensureStream(js); err != nil {
		return err
	}
	s.js = js

	auditConsumer := s.consumerName("audit")
	if _, err := js.QueueSubscribe(
		auditSubject,
		auditConsumer,
		s.handleAudit(ctx),
		nats.BindStream(streamName),
		nats.Durable(auditConsumer),
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
		usageSubject,
		usageConsumer,
		s.handleUsage(ctx),
		nats.BindStream(streamName),
		nats.Durable(usageConsumer),
		nats.ManualAck(),
		nats.AckExplicit(),
		nats.AckWait(30*time.Second),
		nats.MaxDeliver(maxConsumerDeliveries),
		nats.DeliverAll(),
	); err != nil {
		return err
	}
	log.Info().
		Str("audit_subject", auditSubject).
		Str("usage_subject", usageSubject).
		Str("stream", streamName).
		Str("audit_consumer", auditConsumer).
		Str("usage_consumer", usageConsumer).
		Msg("audit-core durable subscribers ready")
	return nil
}

func ensureStream(js nats.JetStreamContext) error {
	config := &nats.StreamConfig{
		Name:       streamName,
		Subjects:   []string{auditSubject, usageSubject, dlqSubject},
		Retention:  nats.LimitsPolicy,
		Storage:    nats.FileStorage,
		Discard:    nats.DiscardOld,
		MaxAge:     30 * 24 * time.Hour,
		Duplicates: 2 * time.Minute,
	}
	if _, err := js.StreamInfo(streamName); err != nil {
		if !errors.Is(err, nats.ErrStreamNotFound) {
			return fmt.Errorf("inspect JetStream stream: %w", err)
		}
		if _, err := js.AddStream(config); err != nil {
			return fmt.Errorf("create JetStream stream: %w", err)
		}
		return nil
	}
	if _, err := js.UpdateStream(config); err != nil {
		return fmt.Errorf("update JetStream stream: %w", err)
	}
	return nil
}

func (s *Subscriber) consumerName(kind string) string {
	return fmt.Sprintf("audit-core-%s-%s", s.bus, kind)
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
		streamSequence, ok := messageStreamSequence(msg)
		if !ok {
			metricsserver.RecordEvent(s.bus, "audit", "metadata_error", ev.OccurredAt)
			s.nak(msg)
			return
		}
		inserted, err := s.store.InsertAuditFromStream(ctx, ev, s.bus, streamSequence)
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
		streamSequence, ok := messageStreamSequence(msg)
		if !ok {
			metricsserver.RecordEvent(s.bus, "usage", "metadata_error", ev.OccurredAt)
			s.nak(msg)
			return
		}
		inserted, err := s.store.InsertUsageFromStream(ctx, ev, s.bus, streamSequence)
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
		}
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
	dlq := nats.NewMsg("velion.dlq.audit-core." + kind)
	dlq.Data = append([]byte(nil), msg.Data...)
	dlq.Header.Set("Velion-Original-Subject", msg.Subject)
	dlq.Header.Set("Velion-Dead-Letter-Reason", reason)
	if _, err := s.js.PublishMsg(dlq); err != nil {
		log.Error().Err(err).Str("subject", msg.Subject).Str("reason", reason).
			Msg("dead-letter publish failed")
		return false
	}
	metricsserver.RecordEvent(s.bus, kind, "dead_lettered", time.Time{})
	return true
}

func messageStreamSequence(msg *nats.Msg) (uint64, bool) {
	metadata, err := msg.Metadata()
	if err != nil || metadata.Sequence.Stream == 0 {
		return 0, false
	}
	return metadata.Sequence.Stream, true
}

// Package subscriber wires NATS subscriptions for the two event
// subject hierarchies that audit-core persists:
//
//   - `velion.audit.v1.<plane>.<event>` — security/operational events.
//   - `velion.usage.v1.<plane>.<op>`    — billable resource usage.
//
// Both are queue-subscribed under a shared group so multiple audit-core
// replicas share the load instead of double-writing.
//
// Failures (bad JSON, store errors) are logged but never NAK'd back to
// NATS — these events are observability data, not state-changing
// operations, and a poison-pill payload should not pile up retries that
// stall the whole subject. If the store is broken, the loss is logged
// and the next event proceeds.
package subscriber

import (
	"context"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/events"
	"github.com/triodelab/controlplane/audit-core/internal/store"
)

const (
	auditSubject = "velion.audit.v1.>"
	usageSubject = "velion.usage.v1.>"
	queueGroup   = "audit-core"
)

type Subscriber struct {
	nc    *nats.Conn
	store *store.Store
}

func New(nc *nats.Conn, s *store.Store) *Subscriber {
	return &Subscriber{nc: nc, store: s}
}

// Start binds the two queue subscriptions. Caller is responsible for
// stopping the underlying NATS connection on shutdown — drain() is the
// natural choice (gracefully completes in-flight handlers).
func (s *Subscriber) Start(ctx context.Context) error {
	if _, err := s.nc.QueueSubscribe(auditSubject, queueGroup, s.handleAudit(ctx)); err != nil {
		return err
	}
	if _, err := s.nc.QueueSubscribe(usageSubject, queueGroup, s.handleUsage(ctx)); err != nil {
		return err
	}
	log.Info().
		Str("audit_subject", auditSubject).
		Str("usage_subject", usageSubject).
		Str("queue_group", queueGroup).
		Msg("audit-core subscribed")
	return nil
}

func (s *Subscriber) handleAudit(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		ev, err := events.DecodeAudit(msg.Data)
		if err != nil {
			log.Warn().Err(err).Str("subject", msg.Subject).Msg("audit: drop malformed event")
			return
		}
		if _, err := s.store.InsertAudit(ctx, ev); err != nil {
			log.Error().Err(err).Str("subject", msg.Subject).Str("org_id", ev.OrgID).
				Msg("audit: insert failed")
		}
	}
}

func (s *Subscriber) handleUsage(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		ev, err := events.DecodeUsage(msg.Data)
		if err != nil {
			log.Warn().Err(err).Str("subject", msg.Subject).Msg("usage: drop malformed event")
			return
		}
		if _, err := s.store.InsertUsage(ctx, ev); err != nil {
			log.Error().Err(err).Str("subject", msg.Subject).Str("org_id", ev.OrgID).
				Msg("usage: insert failed")
		}
	}
}

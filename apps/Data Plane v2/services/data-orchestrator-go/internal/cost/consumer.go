// Package cost consumes embedding/rerank/extraction cost events from NATS
// and persists them to the cost_events table for later querying.
//
// Why orchestrator: this service already owns NATS subscriptions (reindex jobs,
// stale detection) and runs continuously, making it the right place to keep a
// long-lived consumer.
package cost

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"
)

const SubjectCostLedger = "dataplane.cost.ledger"

type Event struct {
	EventType       string `json:"event_type"`
	Model           string `json:"model"`
	Count           int    `json:"count"`
	EstimatedTokens int64  `json:"estimated_tokens"`
	OrgID           string `json:"org_id"`
	// Wave-3.1 §15-F — per-user attribution. Optional; empty string when
	// the cost originated from a background pipeline rather than a user.
	UserID         string `json:"user_id,omitempty"`
	IdempotencyKey string `json:"idempotency_key"`
}

type Consumer struct {
	store    eventStore
	nc       subscriber
	verifier *VerifierRegistry
}

type eventStore interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
}

type subscriber interface {
	Subscribe(string, nats.MsgHandler) (*nats.Subscription, error)
}

func NewSignedConsumer(store eventStore, nc subscriber, verifier *VerifierRegistry) *Consumer {
	return &Consumer{store: store, nc: nc, verifier: verifier}
}

// NewLegacyConsumer is intentionally explicit: callers must separately enforce
// the isolated-development gates before constructing this unsigned consumer.
func NewLegacyConsumer(pool *pgxpool.Pool, nc *nats.Conn) *Consumer {
	return &Consumer{store: pool, nc: nc}
}

// Start subscribes to the cost ledger subject and persists events. Returns
// a cleanup function that unsubscribes when the parent context exits.
func (c *Consumer) Start(ctx context.Context) (func(), error) {
	sub, err := c.nc.Subscribe(SubjectCostLedger, func(msg *nats.Msg) {
		if err := c.processMessage(ctx, msg.Data); err != nil {
			log.Warn().Err(err).Msg("cost ledger: invalid event payload")
		}
	})
	if err != nil {
		return nil, fmt.Errorf("subscribe %s: %w", SubjectCostLedger, err)
	}
	log.Info().Str("subject", SubjectCostLedger).Msg("cost ledger consumer subscribed")
	return func() { _ = sub.Unsubscribe() }, nil
}

func (c *Consumer) processMessage(ctx context.Context, raw []byte) error {
	if c.verifier != nil {
		return c.process(ctx, raw)
	}
	return c.processLegacy(ctx, raw)
}

func (c *Consumer) process(ctx context.Context, raw []byte) error {
	if c.verifier == nil {
		return errInvalidEnvelope
	}
	verified, err := c.verifier.Verify(SubjectCostLedger, raw)
	if err != nil {
		return err
	}
	if verified.ZDR {
		return nil
	}
	if err := c.persist(ctx, &verified.Event); err != nil {
		c.verifier.releaseReplay(verified.replayKey, verified.replayExpiresAt)
		return err
	}
	return nil
}

func (c *Consumer) processLegacy(ctx context.Context, raw []byte) error {
	var legacy struct {
		EventType       string   `json:"event_type"`
		Model           string   `json:"model"`
		Count           int      `json:"count"`
		EstimatedTokens int64    `json:"estimated_tokens"`
		OrgIDs          []string `json:"org_ids"`
		UserID          string   `json:"user_id,omitempty"`
		IdempotencyKey  string   `json:"idempotency_key"`
	}
	if err := json.Unmarshal(raw, &legacy); err != nil {
		return err
	}
	for _, orgID := range legacy.OrgIDs {
		event := Event{
			EventType:       legacy.EventType,
			Model:           legacy.Model,
			Count:           legacy.Count,
			EstimatedTokens: legacy.EstimatedTokens,
			OrgID:           orgID,
			UserID:          legacy.UserID,
			IdempotencyKey:  legacy.IdempotencyKey,
		}
		if err := c.persist(ctx, &event); err != nil {
			return err
		}
	}
	return nil
}

// persist writes one cost event.
//
// Phase 1 RLS: deliberately NOT wrapped in a scope. Two independent reasons:
//
//   - This is a background NATS consumer draining a MIXED-ORG subject — the
//     "worker draining a queue for every org" exception the helper names. One
//     legacy message can even fan out across several orgs (processLegacy loops
//     over evt.OrgIDs), so there is no single tenant this drain belongs to.
//   - More decisively, a scope here would be tautological. The GUC would be set
//     from evt.OrgID and the row's org_id written from that same evt.OrgID, so
//     the policy's WITH CHECK would compare a value against itself and add no
//     isolation whatever. That is the difference from the request-scoped writes
//     in internal/jobs, where the org comes from verified caller claims — an
//     authority independent of the row being written, which is what makes the
//     backstop meaningful.
//
// The tenant boundary here is upstream instead: cost/verifier.go authenticates
// the event's signature before it is ever persisted.
func (c *Consumer) persist(ctx context.Context, evt *Event) error {
	if evt.OrgID == "" {
		return errInvalidEnvelope
	}
	idempotencyKey := evt.IdempotencyKey
	if idempotencyKey != "" {
		idempotencyKey = evt.OrgID + ":" + idempotencyKey
	}
	_, err := c.store.Exec(ctx, `
		INSERT INTO cost_events (event_type, model, org_id, user_id, count, estimated_tokens, idempotency_key)
		VALUES ($1, $2, $3, $4, $5, $6, $7)
		ON CONFLICT (idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING
	`, evt.EventType, evt.Model, evt.OrgID, nilIfEmpty(evt.UserID), evt.Count, evt.EstimatedTokens, nilIfEmpty(idempotencyKey))
	if err != nil {
		return fmt.Errorf("insert cost_event: %w", err)
	}
	return nil
}

func nilIfEmpty(s string) any {
	if s == "" {
		return nil
	}
	return s
}

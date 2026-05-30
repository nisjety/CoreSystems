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

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"
)

const SubjectCostLedger = "dataplane.cost.ledger"

type Event struct {
	EventType       string   `json:"event_type"`
	Model           string   `json:"model"`
	Count           int      `json:"count"`
	EstimatedTokens int64    `json:"estimated_tokens"`
	OrgIDs          []string `json:"org_ids"`
	// Wave-3.1 §15-F — per-user attribution. Optional; empty string when
	// the cost originated from a background pipeline rather than a user.
	UserID         string `json:"user_id,omitempty"`
	IdempotencyKey string `json:"idempotency_key"`
}

type Consumer struct {
	pool *pgxpool.Pool
	nc   *nats.Conn
}

func NewConsumer(pool *pgxpool.Pool, nc *nats.Conn) *Consumer {
	return &Consumer{pool: pool, nc: nc}
}

// Start subscribes to the cost ledger subject and persists events. Returns
// a cleanup function that unsubscribes when the parent context exits.
func (c *Consumer) Start(ctx context.Context) (func(), error) {
	sub, err := c.nc.Subscribe(SubjectCostLedger, func(msg *nats.Msg) {
		var evt Event
		if err := json.Unmarshal(msg.Data, &evt); err != nil {
			log.Warn().Err(err).Msg("cost ledger: invalid event payload")
			return
		}
		if err := c.persist(ctx, &evt); err != nil {
			log.Warn().Err(err).Str("model", evt.Model).Msg("cost ledger: persist failed")
		}
	})
	if err != nil {
		return nil, fmt.Errorf("subscribe %s: %w", SubjectCostLedger, err)
	}
	log.Info().Str("subject", SubjectCostLedger).Msg("cost ledger consumer subscribed")
	return func() { _ = sub.Unsubscribe() }, nil
}

// persist writes one row per (org_id) attached to the event. We split here
// because a single embedding batch can span multiple orgs (rare but legal).
func (c *Consumer) persist(ctx context.Context, evt *Event) error {
	if len(evt.OrgIDs) == 0 {
		return nil
	}
	for _, orgID := range evt.OrgIDs {
		// Per-org idempotency key: scope the publisher's key by org so the
		// unique index discriminates rows correctly when an event spans orgs.
		idemKey := evt.IdempotencyKey
		if idemKey != "" && len(evt.OrgIDs) > 1 {
			idemKey = idemKey + ":" + orgID
		}

		_, err := c.pool.Exec(ctx, `
			INSERT INTO cost_events (event_type, model, org_id, user_id, count, estimated_tokens, idempotency_key)
			VALUES ($1, $2, $3, $4, $5, $6, $7)
			ON CONFLICT (idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING
		`, evt.EventType, evt.Model, orgID, nilIfEmpty(evt.UserID), evt.Count, evt.EstimatedTokens, nilIfEmpty(idemKey))
		if err != nil {
			return fmt.Errorf("insert cost_event: %w", err)
		}
	}
	return nil
}

func nilIfEmpty(s string) any {
	if s == "" {
		return nil
	}
	return s
}

// Package cost consumes the Data Plane's usage events from NATS, records them in the
// cost_events table, and forwards them to cost-core.
//
// Why orchestrator: this service already owns NATS subscriptions (reindex jobs,
// stale detection) and runs continuously, making it the right place to keep a
// long-lived consumer.
//
// The signed consumer reads a durable JetStream stream (stream.go) and acknowledges an
// event only after its row is written, so an event published while this service is down,
// restarting or failing to reach Postgres is read later, exactly once into the table. The
// unsigned consumer (isolated development only) keeps its core subscription.
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

// Event is one cost event as it is recorded. A version 1 event (the count-only event of the
// producers that predate the usage ledger) fills the first block; a version 2 event
// (usage-ledger-rs) fills both.
type Event struct {
	EventType       string `json:"event_type"`
	Model           string `json:"model"`
	Count           int    `json:"count"`
	EstimatedTokens int64  `json:"estimated_tokens"`
	OrgID           string `json:"org_id"`
	// Wave-3.1 §15-F — per-user attribution. Optional; empty string when
	// the cost originated from a background pipeline rather than a user. A usage event
	// (version 2) never has one, and a zero-retention event is recorded without one.
	UserID         string `json:"user_id,omitempty"`
	IdempotencyKey string `json:"idempotency_key"`

	// SchemaVersion is 1 or 2. Feature, ProviderUsed and the rest below are exact for a
	// version 2 event; a version 1 event has no feature of its own (it is recorded under
	// the one its event type implies) and no exact counts.
	SchemaVersion   int
	Feature         string
	ProviderUsed    string
	InputTokens     int64
	OutputTokens    int64
	CacheReadTokens int64
	Units           *int64
	CostBasis       string
	Estimated       bool
	RequestDigest   string
	// ZDR marks an event of a zero-retention organization. It is recorded all the same:
	// the event holds counters only, and the row is written without a user (see persist).
	ZDR bool
}

// Consumer records cost events. The signed consumer (NewSignedConsumer) is the production
// one; the unsigned one exists for isolated development only.
type Consumer struct {
	store    eventStore
	nc       subscriber // the unsigned consumer's core subscription
	conn     *nats.Conn // the signed consumer's connection: it reads the durable stream
	verifier *VerifierRegistry
}

type eventStore interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
}

type subscriber interface {
	Subscribe(string, nats.MsgHandler) (*nats.Subscription, error)
}

// NewSignedConsumer is the production consumer: every event is verified against its
// producer's key and read from the durable stream.
func NewSignedConsumer(store eventStore, conn *nats.Conn, verifier *VerifierRegistry) *Consumer {
	return &Consumer{store: store, conn: conn, verifier: verifier}
}

// NewLegacyConsumer is intentionally explicit: callers must separately enforce
// the isolated-development gates before constructing this unsigned consumer.
func NewLegacyConsumer(pool *pgxpool.Pool, nc *nats.Conn) *Consumer {
	return &Consumer{store: pool, nc: nc}
}

// Start begins consuming and returns a cleanup function that stops it when the parent
// context exits. The signed consumer binds the durable consumer of the cost stream (creating
// the stream if it is the first service to run); the unsigned one subscribes to the subject.
func (c *Consumer) Start(ctx context.Context) (func(), error) {
	if c.verifier != nil {
		return c.startDurable(ctx)
	}
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

// process verifies and records one event that arrived live (not through the durable stream).
func (c *Consumer) process(ctx context.Context, raw []byte) error {
	_, err := c.handle(ctx, raw, false)
	return err
}

// handle verifies one envelope and records it. durable says it was delivered by the stream,
// where an authentic envelope past its own lifetime is expected and accepted.
//
// A zero-retention event is recorded like any other. The earlier convention dropped them,
// which made a zero-retention organization's spend invisible; the event holds counters only
// (usage-ledger-rs has no field for a document, a chunk, a user or text), so recording it
// keeps nothing the organization asked us not to keep.
func (c *Consumer) handle(ctx context.Context, raw []byte, durable bool) (VerifiedCostEvent, error) {
	if c.verifier == nil {
		return VerifiedCostEvent{}, errInvalidEnvelope
	}
	var (
		verified VerifiedCostEvent
		err      error
	)
	if durable {
		verified, err = c.verifier.VerifyDurable(SubjectCostLedger, raw)
	} else {
		verified, err = c.verifier.Verify(SubjectCostLedger, raw)
	}
	if err != nil {
		return VerifiedCostEvent{}, err
	}
	verified.Event.ZDR = verified.ZDR
	if err := c.persist(ctx, &verified.Event); err != nil {
		c.verifier.releaseReplay(verified.replayKey, verified.replayExpiresAt)
		return verified, err
	}
	return verified, nil
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
			SchemaVersion:   1,
			EventType:       legacy.EventType,
			Feature:         v1DefaultFeature[legacy.EventType],
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
//
// Zero-retention rows are "counters only": the row carries the organization, the model, the
// provider, the feature and the numbers, and no user. (The events cannot carry anything
// else: a version 2 event names no user, document or text, and the CHECK of migration
// 20261003140000 refuses a zero-retention row that has a user.)
//
// The idempotency key is stored behind the organization (org:key). The key is only unique
// per producer, and cost-core's unique index is global, so the organization is part of the
// key here and in what is forwarded.
func (c *Consumer) persist(ctx context.Context, evt *Event) error {
	if evt.OrgID == "" {
		return errInvalidEnvelope
	}
	idempotencyKey := evt.IdempotencyKey
	if idempotencyKey != "" {
		idempotencyKey = evt.OrgID + ":" + idempotencyKey
	}
	userID := nilIfEmpty(evt.UserID)
	if evt.ZDR {
		userID = nil
	}
	// The exact columns exist for a version 2 event only: a version 1 row says "unknown"
	// (NULL), never a zero that would read as "free".
	var inputTokens, outputTokens, cacheReadTokens, units, costBasis, estimated, requestID any
	if evt.SchemaVersion >= schemaVersionUsage {
		inputTokens, outputTokens, cacheReadTokens = evt.InputTokens, evt.OutputTokens, evt.CacheReadTokens
		if evt.Units != nil {
			units = *evt.Units
		}
		costBasis, estimated, requestID = nilIfEmpty(evt.CostBasis), evt.Estimated, nilIfEmpty(evt.RequestDigest)
	}
	schemaVersion := evt.SchemaVersion
	if schemaVersion == 0 {
		schemaVersion = 1
	}
	_, err := c.store.Exec(ctx, insertCostEventSQL,
		evt.EventType, evt.Model, evt.OrgID, userID, evt.Count, evt.EstimatedTokens, nilIfEmpty(idempotencyKey),
		schemaVersion, nilIfEmpty(evt.Feature), nilIfEmpty(evt.ProviderUsed),
		inputTokens, outputTokens, cacheReadTokens, units, costBasis, estimated, evt.ZDR, requestID)
	if err != nil {
		return fmt.Errorf("insert cost_event: %w", err)
	}
	return nil
}

// insertCostEventSQL is the one statement that writes a cost event. The first seven
// arguments are the version 1 columns in their original order.
const insertCostEventSQL = `
		INSERT INTO cost_events (event_type, model, org_id, user_id, count, estimated_tokens, idempotency_key,
		                         schema_version, feature, provider_used,
		                         input_tokens, output_tokens, cache_read_tokens, units, cost_basis, estimated, zdr, request_id)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
		ON CONFLICT (idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING
	`

func nilIfEmpty(s string) any {
	if s == "" {
		return nil
	}
	return s
}

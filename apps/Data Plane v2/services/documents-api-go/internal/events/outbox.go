// Package events — §16.2.6 outbox publisher loop.
//
// BulkIngest commits the document row AND an outbox row in the same tx.
// This goroutine polls the outbox every `pollInterval`, publishes the
// payload to NATS, then marks the row published. Crash between commit
// and publish is safe — the next tick re-emits.
//
// at-least-once delivery: consumers must be idempotent (which ours are —
// Qdrant upserts are keyed by knowledge_id, etc.).

package events

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"
)

const (
	// Default poll interval — fast enough to feel real-time, slow enough
	// not to hammer Postgres when the outbox is empty.
	defaultOutboxPollInterval = 500 * time.Millisecond
	// Cap the per-tick batch so a runaway producer doesn't starve other
	// queries on the connection pool.
	maxOutboxBatchSize     = 200
	sourceObjectStreamName = "DATAPLANE_SOURCE_OBJECTS"
)

type OutboxPublisher struct {
	pool         *pgxpool.Pool
	js           jetStreamPublisher
	pollInterval time.Duration
	signer       interface {
		Sign(eventType string, payload []byte) ([]byte, error)
	}
}

type jetStreamPublisher interface {
	Publish(subject string, data []byte, opts ...nats.PubOpt) (*nats.PubAck, error)
}

type sourceStreamManager interface {
	StreamInfo(stream string, opts ...nats.JSOpt) (*nats.StreamInfo, error)
	AddStream(cfg *nats.StreamConfig, opts ...nats.JSOpt) (*nats.StreamInfo, error)
}

func NewOutboxPublisher(pool *pgxpool.Pool, nc *nats.Conn, signer interface {
	Sign(eventType string, payload []byte) ([]byte, error)
}) (*OutboxPublisher, error) {
	if pool == nil || nc == nil || signer == nil {
		return nil, fmt.Errorf("documents outbox requires postgres, NATS, and event signer")
	}
	js, err := nc.JetStream()
	if err != nil {
		return nil, fmt.Errorf("initialize JetStream outbox publisher: %w", err)
	}
	if err := ensureSourceObjectStream(js); err != nil {
		return nil, err
	}
	return &OutboxPublisher{
		pool:         pool,
		js:           js,
		pollInterval: defaultOutboxPollInterval,
		signer:       signer,
	}, nil
}

func sourceObjectStreamConfig() nats.StreamConfig {
	return nats.StreamConfig{
		Name:        sourceObjectStreamName,
		Description: "Bounded durable source-object lifecycle events owned by documents-api",
		Subjects: []string{
			SubjectSourceObjectChanged,
			SubjectSourceObjectDeleted,
		},
		Retention:  nats.LimitsPolicy,
		MaxMsgs:    100_000,
		MaxBytes:   256 * 1024 * 1024,
		Discard:    nats.DiscardOld,
		MaxAge:     7 * 24 * time.Hour,
		MaxMsgSize: 1024 * 1024,
		Storage:    nats.FileStorage,
		Replicas:   1,
		Duplicates: 10 * time.Minute,
		DenyDelete: true,
		DenyPurge:  true,
	}
}

func ensureSourceObjectStream(js sourceStreamManager) error {
	if js == nil {
		return fmt.Errorf("source-object JetStream manager is unavailable")
	}
	want := sourceObjectStreamConfig()
	info, err := js.StreamInfo(want.Name)
	if err != nil {
		if !errors.Is(err, nats.ErrStreamNotFound) {
			return fmt.Errorf("inspect source-object stream: %w", err)
		}
		info, err = js.AddStream(&want)
		if err != nil {
			// Multiple replicas may race to create the same owned stream. Only
			// accept the race when the resulting stream is present and safe.
			info, err = js.StreamInfo(want.Name)
			if err != nil {
				return fmt.Errorf("create source-object stream: %w", err)
			}
		}
	}
	if info == nil {
		return fmt.Errorf("source-object stream inspection returned no configuration")
	}
	if err := validateSourceObjectStream(info.Config, want); err != nil {
		return err
	}
	return nil
}

func validateSourceObjectStream(got, want nats.StreamConfig) error {
	if got.Name != want.Name || !reflect.DeepEqual(got.Subjects, want.Subjects) ||
		got.Retention != want.Retention || got.MaxMsgs != want.MaxMsgs ||
		got.MaxBytes != want.MaxBytes || got.Discard != want.Discard ||
		got.MaxAge != want.MaxAge || got.MaxMsgSize != want.MaxMsgSize ||
		got.Storage != want.Storage || got.Replicas != want.Replicas ||
		got.Duplicates != want.Duplicates || got.DenyDelete != want.DenyDelete ||
		got.DenyPurge != want.DenyPurge {
		return fmt.Errorf("source-object stream configuration is unsafe or incompatible")
	}
	return nil
}

// Start kicks off the background loop. Returns immediately; honors ctx
// cancellation for graceful shutdown.
func (p *OutboxPublisher) Start(ctx context.Context) {
	if p == nil || p.js == nil {
		panic("documents outbox started without acknowledged JetStream publisher")
	}
	go p.loop(ctx)
}

func (p *OutboxPublisher) loop(ctx context.Context) {
	ticker := time.NewTicker(p.pollInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if err := p.drainOnce(ctx); err != nil {
				fmt.Printf("warn: outbox drain failed: %v\n", err)
			}
		}
	}
}

// drainOnce reads up to maxOutboxBatchSize unpublished rows, publishes
// each, then marks them published. We use a single tx + SKIP LOCKED so
// multiple replicas can run this loop without double-publishing.
func (p *OutboxPublisher) drainOnce(ctx context.Context) error {
	tx, err := p.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin: %w", err)
	}
	defer tx.Rollback(ctx)

	rows, err := tx.Query(ctx, `
		SELECT outbox_id, event_type, payload
		FROM documents_outbox
		WHERE published = FALSE
		ORDER BY created_at
		LIMIT $1
		FOR UPDATE SKIP LOCKED
	`, maxOutboxBatchSize)
	if err != nil {
		return fmt.Errorf("select outbox: %w", err)
	}

	type row struct {
		id        int64
		eventType string
		payload   json.RawMessage
	}
	var batch []row
	for rows.Next() {
		var r row
		if err := rows.Scan(&r.id, &r.eventType, &r.payload); err != nil {
			rows.Close()
			return fmt.Errorf("scan outbox row: %w", err)
		}
		batch = append(batch, r)
	}
	rows.Close()

	if len(batch) == 0 {
		return tx.Commit(ctx)
	}

	publishedIDs := make([]int64, 0, len(batch))
	for _, r := range batch {
		// `event_type` doubles as the NATS subject — the outbox writer
		// chose it deliberately. This keeps the publisher contract-free.
		if err := p.publishAcknowledged(r.eventType, r.payload, fmt.Sprintf("documents-outbox-%d", r.id)); err != nil {
			fmt.Printf("warn: signed JetStream publish %s: %v\n", r.eventType, err)
			continue
		}
		publishedIDs = append(publishedIDs, r.id)
	}

	if len(publishedIDs) == 0 {
		return tx.Commit(ctx)
	}

	if _, err := tx.Exec(ctx, `
		UPDATE documents_outbox
		SET published = TRUE, published_at = NOW()
		WHERE outbox_id = ANY($1)
	`, publishedIDs); err != nil {
		return fmt.Errorf("mark published: %w", err)
	}
	return tx.Commit(ctx)
}

func (p *OutboxPublisher) publishAcknowledged(eventType string, payload []byte, messageID string) error {
	if p == nil || p.signer == nil || p.js == nil {
		return fmt.Errorf("signed acknowledged outbox publisher unavailable")
	}
	envelope, err := p.signer.Sign(eventType, payload)
	if err != nil {
		return fmt.Errorf("sign event: %w", err)
	}
	if messageID == "" {
		return fmt.Errorf("stable outbox message id is required")
	}
	ack, err := p.js.Publish(eventType, envelope, nats.MsgId(messageID))
	if err != nil {
		return fmt.Errorf("JetStream publish acknowledgement: %w", err)
	}
	if ack == nil || ack.Stream == "" {
		return fmt.Errorf("JetStream publish returned an invalid acknowledgement")
	}
	return nil
}

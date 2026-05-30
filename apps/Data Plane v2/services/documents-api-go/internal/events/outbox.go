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
	"fmt"
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
	maxOutboxBatchSize = 200
)

type OutboxPublisher struct {
	pool         *pgxpool.Pool
	nc           *nats.Conn
	pollInterval time.Duration
}

func NewOutboxPublisher(pool *pgxpool.Pool, nc *nats.Conn) *OutboxPublisher {
	return &OutboxPublisher{
		pool:         pool,
		nc:           nc,
		pollInterval: defaultOutboxPollInterval,
	}
}

// Start kicks off the background loop. Returns immediately; honors ctx
// cancellation for graceful shutdown.
func (p *OutboxPublisher) Start(ctx context.Context) {
	if p == nil || p.nc == nil {
		// Disabled — no NATS attached. Outbox rows accumulate and a
		// later operator can drain via SQL.
		return
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
		if err := p.nc.Publish(r.eventType, r.payload); err != nil {
			fmt.Printf("warn: nats publish %s: %v\n", r.eventType, err)
			continue
		}
		publishedIDs = append(publishedIDs, r.id)
	}
	if err := p.nc.Flush(); err != nil {
		fmt.Printf("warn: nats flush: %v\n", err)
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

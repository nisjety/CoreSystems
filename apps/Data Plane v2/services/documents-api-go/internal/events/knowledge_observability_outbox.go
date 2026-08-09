package events

import (
	"context"
	"fmt"
	"log"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// KnowledgeObservabilityOutbox mirrors the already-durable document lifecycle
// outbox into the Application Plane only after the local document event has
// been published. It has its own completion marker so an Insights outage never
// delays indexing, while an Insights restart can replay rows idempotently.
type KnowledgeObservabilityOutbox struct {
	pool         *pgxpool.Pool
	publisher    knowledgeObservationPublisher
	pollInterval time.Duration
}

func NewKnowledgeObservabilityOutbox(pool *pgxpool.Pool, publisher knowledgeObservationPublisher) (*KnowledgeObservabilityOutbox, error) {
	if pool == nil || publisher == nil {
		return nil, fmt.Errorf("knowledge observability outbox requires postgres and an application-plane publisher")
	}
	return &KnowledgeObservabilityOutbox{
		pool:         pool,
		publisher:    publisher,
		pollInterval: defaultOutboxPollInterval,
	}, nil
}

func (p *KnowledgeObservabilityOutbox) Start(ctx context.Context) {
	if p == nil || p.pool == nil || p.publisher == nil {
		panic("knowledge observability outbox started without dependencies")
	}
	go p.loop(ctx)
}

func (p *KnowledgeObservabilityOutbox) loop(ctx context.Context) {
	ticker := time.NewTicker(p.pollInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if err := p.drainOnce(ctx); err != nil {
				log.Printf("warn: knowledge observability outbox drain failed: %v", err)
			}
		}
	}
}

// drainOnce mirrors already-published outbox rows into the Application Plane.
//
// Phase 1 RLS: deliberately UNSCOPED, for the same reason as
// OutboxPublisher.drainOnce in outbox.go — this is a cross-org background loop
// with no org_id filter, and a scoped transaction would silently narrow it to a
// single tenant while still looking healthy. Note that each row it reads
// carries its own org_id, which it forwards to the publisher; that per-row
// value is telemetry, not a scope the loop as a whole could adopt.
func (p *KnowledgeObservabilityOutbox) drainOnce(ctx context.Context) error {
	tx, err := p.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin knowledge observability outbox: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck

	rows, err := tx.Query(ctx, `
		SELECT outbox_id, org_id, event_type, payload, created_at
		FROM documents_outbox
		WHERE published = TRUE AND observability_published = FALSE
		ORDER BY created_at
		LIMIT $1
		FOR UPDATE SKIP LOCKED
	`, maxOutboxBatchSize)
	if err != nil {
		return fmt.Errorf("select knowledge observability outbox: %w", err)
	}
	defer rows.Close()

	batch := make([]knowledgeOutboxRow, 0, maxOutboxBatchSize)
	for rows.Next() {
		var row knowledgeOutboxRow
		if err := rows.Scan(&row.ID, &row.OrgID, &row.EventType, &row.Payload, &row.CreatedAt); err != nil {
			return fmt.Errorf("scan knowledge observability outbox: %w", err)
		}
		batch = append(batch, row)
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("iterate knowledge observability outbox: %w", err)
	}
	if len(batch) == 0 {
		return tx.Commit(ctx)
	}

	completed, err := publishKnowledgeObservations(p.publisher, batch)
	if err != nil {
		return err
	}
	if len(completed) == 0 {
		return tx.Commit(ctx)
	}
	if _, err := tx.Exec(ctx, `
		UPDATE documents_outbox
		SET observability_published = TRUE, observability_published_at = NOW()
		WHERE outbox_id = ANY($1)
	`, completed); err != nil {
		return fmt.Errorf("mark knowledge observations published: %w", err)
	}
	return tx.Commit(ctx)
}

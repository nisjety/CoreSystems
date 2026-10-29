package events

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

const (
	defaultOutboxPollInterval = 500 * time.Millisecond
	defaultOutboxLease        = 30 * time.Second
	maxOutboxBatchSize        = 100
)

type OutboxRow struct {
	ID        int64
	EventType string
	Payload   []byte
	Attempts  int
}

type outboxStore interface {
	Claim(context.Context, string, int, time.Duration) ([]OutboxRow, error)
	MarkDelivered(context.Context, int64, string) error
	Retry(context.Context, int64, string, time.Duration, string) error
}

type acknowledgedPublisher interface {
	PublishAcknowledged(string, []byte, string) error
}

type OutboxPublisher struct {
	store        outboxStore
	publisher    acknowledgedPublisher
	owner        string
	pollInterval time.Duration
	lease        time.Duration
}

func NewOutboxPublisher(pool *pgxpool.Pool, publisher *Publisher) (*OutboxPublisher, error) {
	if pool == nil || publisher == nil || publisher.js == nil || publisher.signer == nil {
		return nil, fmt.Errorf("wiki outbox requires postgres and acknowledged signed publisher")
	}
	return newOutboxPublisher(
		&postgresOutboxStore{pool: pool},
		publisher,
		"wiki-store:"+uuid.NewString(),
		defaultOutboxPollInterval,
	), nil
}

func newOutboxPublisher(store outboxStore, publisher acknowledgedPublisher, owner string, poll time.Duration) *OutboxPublisher {
	return &OutboxPublisher{
		store: store, publisher: publisher, owner: owner,
		pollInterval: poll, lease: defaultOutboxLease,
	}
}

func (p *OutboxPublisher) Start(ctx context.Context) {
	if p == nil || p.store == nil || p.publisher == nil || p.owner == "" {
		panic("wiki outbox started without secure dependencies")
	}
	go p.loop(ctx)
}

func (p *OutboxPublisher) loop(ctx context.Context) {
	if err := p.drainOnce(ctx); err != nil {
		fmt.Printf("warn: wiki outbox drain failed: %v\n", err)
	}
	ticker := time.NewTicker(p.pollInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if err := p.drainOnce(ctx); err != nil {
				fmt.Printf("warn: wiki outbox drain failed: %v\n", err)
			}
		}
	}
}

func (p *OutboxPublisher) drainOnce(ctx context.Context) error {
	rows, err := p.store.Claim(ctx, p.owner, maxOutboxBatchSize, p.lease)
	if err != nil {
		return fmt.Errorf("claim wiki outbox: %w", err)
	}
	for _, row := range rows {
		if err := p.publisher.PublishAcknowledged(row.EventType, row.Payload, fmt.Sprintf("wiki-outbox-%d", row.ID)); err != nil {
			retryAfter := retryDelay(row.Attempts)
			if retryErr := p.store.Retry(ctx, row.ID, p.owner, retryAfter, sanitizeOutboxError(err)); retryErr != nil {
				return fmt.Errorf("requeue wiki outbox %d: %w", row.ID, retryErr)
			}
			continue
		}
		if err := p.store.MarkDelivered(ctx, row.ID, p.owner); err != nil {
			return fmt.Errorf("mark wiki outbox %d delivered: %w", row.ID, err)
		}
	}
	return nil
}

func retryDelay(attempts int) time.Duration {
	if attempts < 1 {
		attempts = 1
	}
	shift := min(attempts-1, 8)
	return time.Duration(1<<shift) * time.Second
}

func sanitizeOutboxError(err error) string {
	message := strings.TrimSpace(err.Error())
	if len(message) > 512 {
		message = message[:512]
	}
	return message
}

// postgresOutboxStore is the cross-org drain behind OutboxPublisher's poll
// loop.
//
// Phase 1 RLS: every query in this type deliberately stays on the unscoped
// pool. One wiki-store-go process drains the outbox for EVERY org, so a
// scoped transaction would narrow Claim to a single tenant's rows and
// silently stop publishing everyone else's wiki events — a failure with no
// error to notice, since a short claim batch is indistinguishable from an
// idle queue. MarkDelivered and Retry act on outbox_id values Claim already
// returned, so they are equally org-agnostic by construction.
//
// The org boundary on the write side is enforced where the rows are created
// instead: enqueueWikiPublished (internal/repo/wiki_repo.go) inserts each
// intent inside the publishing org's scoped transaction, and the table's
// wiki_event_outbox_payload_check constraint requires payload->>'org_id' to
// equal the row's org_id — so a row cannot be enqueued under the wrong
// tenant for this loop to later pick up.
type postgresOutboxStore struct {
	pool *pgxpool.Pool
}

func (s *postgresOutboxStore) Claim(ctx context.Context, owner string, limit int, lease time.Duration) ([]OutboxRow, error) {
	rows, err := s.pool.Query(ctx, `
		WITH candidates AS (
			SELECT outbox_id
			FROM wiki_event_outbox
			WHERE (status = 'pending' AND available_at <= NOW())
			   OR (status = 'leased' AND lease_until <= NOW())
			ORDER BY available_at, outbox_id
			LIMIT $3
			FOR UPDATE SKIP LOCKED
		)
		UPDATE wiki_event_outbox AS outbox
		SET status = 'leased', lease_owner = $1,
			lease_until = NOW() + make_interval(secs => $2),
			attempts = outbox.attempts + 1, updated_at = NOW()
		FROM candidates
		WHERE outbox.outbox_id = candidates.outbox_id
		RETURNING outbox.outbox_id, outbox.event_type, outbox.payload, outbox.attempts
	`, owner, int64(lease/time.Second), limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	claimed := make([]OutboxRow, 0, limit)
	for rows.Next() {
		var row OutboxRow
		if err := rows.Scan(&row.ID, &row.EventType, &row.Payload, &row.Attempts); err != nil {
			return nil, err
		}
		claimed = append(claimed, row)
	}
	return claimed, rows.Err()
}

func (s *postgresOutboxStore) MarkDelivered(ctx context.Context, id int64, owner string) error {
	result, err := s.pool.Exec(ctx, `
		UPDATE wiki_event_outbox
		SET status = 'delivered', delivered_at = NOW(), lease_owner = NULL,
			lease_until = NULL, last_error = NULL, updated_at = NOW()
		WHERE outbox_id = $1 AND status = 'leased' AND lease_owner = $2
	`, id, owner)
	if err != nil {
		return err
	}
	if result.RowsAffected() != 1 {
		return fmt.Errorf("wiki outbox lease lost")
	}
	return nil
}

func (s *postgresOutboxStore) Retry(ctx context.Context, id int64, owner string, delay time.Duration, message string) error {
	result, err := s.pool.Exec(ctx, `
		UPDATE wiki_event_outbox
		SET status = 'pending', available_at = NOW() + make_interval(secs => $3),
			lease_owner = NULL, lease_until = NULL, last_error = $4, updated_at = NOW()
		WHERE outbox_id = $1 AND status = 'leased' AND lease_owner = $2
	`, id, owner, int64(delay/time.Second), message)
	if err != nil {
		return err
	}
	if result.RowsAffected() != 1 {
		return fmt.Errorf("wiki outbox lease lost")
	}
	return nil
}

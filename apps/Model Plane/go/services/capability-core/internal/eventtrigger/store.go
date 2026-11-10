package eventtrigger

import (
	"context"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// recorder writes the pending fires one webhook event creates.
type recorder interface {
	record(ctx context.Context, event WebhookReceived) (int, error)
}

// ledger opens a sweep batch over the fire table.
type ledger interface {
	begin(ctx context.Context) (batch, error)
}

// batch is one transaction of sweep work. The claimed fires, the counts a decision reads and the settlements commit
// together, so a failure leaves every fire of the batch pending for the next sweep.
type batch interface {
	due(ctx context.Context, limit int) ([]Fire, error)
	counts(ctx context.Context, fire Fire, since time.Time) (Counts, error)
	settle(ctx context.Context, fire Fire, settlement Settlement, now time.Time) error
	commit(ctx context.Context) error
	rollback(ctx context.Context)
}

// pgStore is the Postgres fire table. It reads the session-core trigger table in the same database, as the cron
// sweeper reads the cron tables. wakeFilter is set only when the wake gate is on (Sweeper.EnableWakeGate), so a sweeper
// without the gate never reads the wake filter column.
type pgStore struct {
	pool       *pgxpool.Pool
	wakeFilter bool
}

// record writes one pending fire for each active trigger bound to the event's connection, in one transaction. A
// trigger that already has a fire for this webhook event keeps it, so a redelivered event writes nothing new.
func (s pgStore) record(ctx context.Context, event WebhookReceived) (int, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return 0, fmt.Errorf("begin trigger fire record: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	rows, err := tx.Query(ctx, `
		SELECT id FROM event_triggers
		WHERE org_id = $1 AND connection_id = $2 AND status = 'active'
		ORDER BY created_at, id`, event.OrgID, event.ConnectionID)
	if err != nil {
		return 0, fmt.Errorf("match event triggers: %w", err)
	}
	var triggerIDs []string
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			rows.Close()
			return 0, fmt.Errorf("scan event trigger: %w", err)
		}
		triggerIDs = append(triggerIDs, id)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return 0, fmt.Errorf("read event triggers: %w", err)
	}

	recorded := 0
	for _, triggerID := range triggerIDs {
		tag, err := tx.Exec(ctx, `
			INSERT INTO event_trigger_fires (id, trigger_id, org_id, webhook_event_id, status)
			VALUES ($1, $2, $3, $4, 'pending')
			ON CONFLICT (trigger_id, webhook_event_id) DO NOTHING`,
			"trgfire_"+uuid.New().String(), triggerID, event.OrgID, event.WebhookEventID)
		if err != nil {
			return 0, fmt.Errorf("record trigger fire: %w", err)
		}
		recorded += int(tag.RowsAffected())
	}
	if err := tx.Commit(ctx); err != nil {
		return 0, fmt.Errorf("commit trigger fires: %w", err)
	}
	return recorded, nil
}

func (s pgStore) begin(ctx context.Context) (batch, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin trigger fire sweep: %w", err)
	}
	return pgBatch{tx: tx, wakeFilter: s.wakeFilter}, nil
}

type pgBatch struct {
	tx         pgx.Tx
	wakeFilter bool
}

// The claim's SQL either side of the wake filter column. The column is named only when the wake gate is on, so a sweeper
// without the gate reads nothing it does not need.
const dueFiresSelect = `
		SELECT f.id, f.trigger_id, f.org_id, f.webhook_event_id, f.attempts,
		       t.connection_id, t.space_ref, t.member_user_id, t.status, `

const dueFiresFrom = `
		FROM event_trigger_fires f
		JOIN event_triggers t ON t.id = f.trigger_id
		WHERE f.status = 'pending'
		ORDER BY f.created_at, f.id
		LIMIT $1
		FOR UPDATE OF f SKIP LOCKED`

// due claims up to limit pending fires. A row another replica holds is skipped, so replicas never decide one fire
// twice. The stored wake filter is read only when the wake gate is on; with it off every fire's filter is nil.
func (b pgBatch) due(ctx context.Context, limit int) ([]Fire, error) {
	wakeFilter := "NULL::jsonb"
	if b.wakeFilter {
		wakeFilter = "t.wake_filter"
	}
	rows, err := b.tx.Query(ctx, dueFiresSelect+wakeFilter+dueFiresFrom, limit)
	if err != nil {
		return nil, fmt.Errorf("claim pending trigger fires: %w", err)
	}
	defer rows.Close()
	var fires []Fire
	for rows.Next() {
		var fire Fire
		if err := rows.Scan(&fire.ID, &fire.TriggerID, &fire.OrgID, &fire.WebhookEventID, &fire.Attempts,
			&fire.ConnectionID, &fire.SpaceRef, &fire.MemberUserID, &fire.TriggerStatus, &fire.WakeFilter); err != nil {
			return nil, fmt.Errorf("scan pending trigger fire: %w", err)
		}
		fires = append(fires, fire)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read pending trigger fires: %w", err)
	}
	return fires, nil
}

// counts reads how many fires the fire's trigger and its org have been authorised for since the window opened. It
// sees this batch's own earlier settlements, because they are in the same transaction.
func (b pgBatch) counts(ctx context.Context, fire Fire, since time.Time) (Counts, error) {
	var trigger, org int64
	err := b.tx.QueryRow(ctx, `
		SELECT COUNT(*) FILTER (WHERE trigger_id = $3), COUNT(*)
		FROM event_trigger_fires
		WHERE org_id = $1 AND status = 'authorized' AND settled_at >= $2`,
		fire.OrgID, since, fire.TriggerID).Scan(&trigger, &org)
	if err != nil {
		return Counts{}, fmt.Errorf("count authorised trigger fires: %w", err)
	}
	return Counts{Trigger: int(trigger), Org: int(org)}, nil
}

func (b pgBatch) settle(ctx context.Context, fire Fire, settlement Settlement, now time.Time) error {
	if _, err := b.tx.Exec(ctx, `
		UPDATE event_trigger_fires
		SET status = $2, outcome = $3, attempts = $4,
		    settled_at = CASE WHEN $2 = 'pending' THEN NULL ELSE $5::timestamptz END
		WHERE id = $1 AND status = 'pending'`,
		fire.ID, settlement.Status, settlement.Outcome, settlement.Attempts, now); err != nil {
		return fmt.Errorf("settle trigger fire: %w", err)
	}
	return nil
}

func (b pgBatch) commit(ctx context.Context) error {
	if err := b.tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit trigger fire sweep: %w", err)
	}
	return nil
}

func (b pgBatch) rollback(ctx context.Context) {
	_ = b.tx.Rollback(ctx)
}

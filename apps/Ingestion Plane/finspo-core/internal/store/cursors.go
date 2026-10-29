package store

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Cursor mirrors one row of delta_cursors.
type Cursor struct {
	SourceID       uuid.UUID  `json:"source_id"`
	DeltaToken     string     `json:"delta_token,omitempty"`
	DeltaLink      string     `json:"delta_link,omitempty"`
	LastSyncedAt   *time.Time `json:"last_synced_at,omitempty"`
	LastStatus     string     `json:"last_status,omitempty"`
	LastError      string     `json:"last_error,omitempty"`
	ItemsSeenTotal int64      `json:"items_seen_total"`
	UpdatedAt      time.Time  `json:"updated_at"`
}

type Cursors struct {
	pool *pgxpool.Pool
}

// Get returns the persisted cursor for a source, or ErrNotFound when the
// source has never been synced.
func (c *Cursors) Get(ctx context.Context, sourceID uuid.UUID) (Cursor, error) {
	const q = `
SELECT source_id, COALESCE(delta_token,''), COALESCE(delta_link,''),
       last_synced_at, COALESCE(last_status,''), COALESCE(last_error,''),
       items_seen_total, updated_at
  FROM delta_cursors
 WHERE source_id = $1`
	var out Cursor
	err := c.pool.QueryRow(ctx, q, sourceID).Scan(
		&out.SourceID, &out.DeltaToken, &out.DeltaLink,
		&out.LastSyncedAt, &out.LastStatus, &out.LastError,
		&out.ItemsSeenTotal, &out.UpdatedAt,
	)
	if errors.Is(err, pgxNoRows) {
		return Cursor{}, ErrNotFound
	}
	if err != nil {
		return Cursor{}, fmt.Errorf("get cursor: %w", err)
	}
	return out, nil
}

// Save upserts the cursor row. itemsDelta is added to items_seen_total; pass 0
// to leave the counter unchanged.
func (c *Cursors) Save(ctx context.Context, sourceID uuid.UUID, deltaLink, status, errMsg string, itemsDelta int64) (Cursor, error) {
	const q = `
INSERT INTO delta_cursors
    (source_id, delta_link, last_synced_at, last_status, last_error, items_seen_total)
VALUES ($1, NULLIF($2,''), NOW(), NULLIF($3,''), NULLIF($4,''), $5)
ON CONFLICT (source_id) DO UPDATE
   SET delta_link       = COALESCE(NULLIF(EXCLUDED.delta_link,''), delta_cursors.delta_link),
       last_synced_at   = EXCLUDED.last_synced_at,
       last_status      = EXCLUDED.last_status,
       last_error       = EXCLUDED.last_error,
       items_seen_total = delta_cursors.items_seen_total + EXCLUDED.items_seen_total
RETURNING source_id, COALESCE(delta_token,''), COALESCE(delta_link,''),
          last_synced_at, COALESCE(last_status,''), COALESCE(last_error,''),
          items_seen_total, updated_at`
	var out Cursor
	err := c.pool.QueryRow(ctx, q, sourceID, deltaLink, status, errMsg, itemsDelta).Scan(
		&out.SourceID, &out.DeltaToken, &out.DeltaLink,
		&out.LastSyncedAt, &out.LastStatus, &out.LastError,
		&out.ItemsSeenTotal, &out.UpdatedAt,
	)
	if err != nil {
		return Cursor{}, fmt.Errorf("save cursor: %w", err)
	}
	return out, nil
}

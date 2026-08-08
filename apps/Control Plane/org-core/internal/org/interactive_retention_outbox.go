package org

import (
	"context"
	"fmt"
)

func (r *Repository) ClaimInteractiveRetentionOutbox(ctx context.Context, limit int) ([]InteractiveRetentionOutboxRow, error) {
	if limit < 1 || limit > 1000 {
		return nil, fmt.Errorf("interactive retention outbox limit must be between 1 and 1000")
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin interactive retention outbox claim: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	rows, err := tx.Query(ctx, `
WITH claimed AS (
  SELECT event_id
  FROM organization_interactive_retention_outbox
  WHERE published_at IS NULL
    AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '1 minute')
  ORDER BY created_at, event_id
  LIMIT $1
  FOR UPDATE SKIP LOCKED
)
UPDATE organization_interactive_retention_outbox o
SET processing_at = NOW(), updated_at = NOW()
FROM claimed
WHERE o.event_id = claimed.event_id
RETURNING o.event_id, o.org_id, o.attempts`, limit)
	if err != nil {
		return nil, fmt.Errorf("claim interactive retention outbox: %w", err)
	}
	defer rows.Close()
	claimed := make([]InteractiveRetentionOutboxRow, 0)
	for rows.Next() {
		var row InteractiveRetentionOutboxRow
		if err := rows.Scan(&row.EventID, &row.OrgID, &row.Attempts); err != nil {
			return nil, fmt.Errorf("scan interactive retention outbox: %w", err)
		}
		claimed = append(claimed, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate interactive retention outbox: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit interactive retention outbox claim: %w", err)
	}
	return claimed, nil
}

func (r *Repository) MarkInteractiveRetentionPublished(ctx context.Context, eventID int64) error {
	result, err := r.pool.Exec(ctx, `
UPDATE organization_interactive_retention_outbox
SET published_at = NOW(), processing_at = NULL, attempts = attempts + 1,
    last_error = NULL, updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL`, eventID)
	if err != nil {
		return fmt.Errorf("mark interactive retention published: %w", err)
	}
	if result.RowsAffected() != 1 {
		return fmt.Errorf("interactive retention acknowledgement did not match a pending event")
	}
	return nil
}

func (r *Repository) MarkInteractiveRetentionPublishFailed(ctx context.Context, eventID int64, publishErr error) error {
	message := "unknown interactive retention publish failure"
	if publishErr != nil {
		message = publishErr.Error()
	}
	if len(message) > 2048 {
		message = message[:2048]
	}
	_, err := r.pool.Exec(ctx, `
UPDATE organization_interactive_retention_outbox
SET processing_at = NULL, attempts = attempts + 1,
    last_error = $2, updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL`, eventID, message)
	if err != nil {
		return fmt.Errorf("record interactive retention publish failure: %w", err)
	}
	return nil
}

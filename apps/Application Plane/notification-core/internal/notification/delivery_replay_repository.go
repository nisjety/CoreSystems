package notification

import (
	"context"
	"fmt"
	"strings"
	"time"
)

// Claim implements DeliveryCallbackReplayStore with Postgres rather than a
// process-local map, so callback replay protection survives replica restarts.
// Expired nonces may be reclaimed atomically; live nonces are never reused.
func (r *PGRepository) Claim(ctx context.Context, nonce string, expiresAt time.Time) (bool, error) {
	if r == nil || r.pool == nil {
		return false, fmt.Errorf("notification repository is not configured")
	}
	nonce = strings.TrimSpace(nonce)
	expiresAt = expiresAt.UTC()
	if nonce == "" || expiresAt.IsZero() || !expiresAt.After(time.Now().UTC()) {
		return false, fmt.Errorf("callback nonce and future expiry are required")
	}
	var claimed bool
	err := r.pool.QueryRow(ctx, `
WITH inserted AS (
	INSERT INTO notification_delivery_callback_replays (nonce, claimed_at, expires_at)
	VALUES ($1, NOW(), $2)
	ON CONFLICT (nonce) DO NOTHING
	RETURNING nonce
), reclaimed AS (
	UPDATE notification_delivery_callback_replays
	SET claimed_at = NOW(), expires_at = $2
	WHERE nonce = $1
	  AND expires_at <= NOW()
	  AND NOT EXISTS (SELECT 1 FROM inserted)
	RETURNING nonce
)
SELECT EXISTS (
	SELECT nonce FROM inserted
	UNION ALL
	SELECT nonce FROM reclaimed
)`, nonce, expiresAt).Scan(&claimed)
	return claimed, err
}

func (r *PGRepository) DeleteExpiredDeliveryCallbackReplays(ctx context.Context, now time.Time, limit int) (int64, error) {
	if r == nil || r.pool == nil {
		return 0, fmt.Errorf("notification repository is not configured")
	}
	if limit < 1 {
		return 0, fmt.Errorf("positive replay cleanup limit is required")
	}
	tag, err := r.pool.Exec(ctx, `
WITH expired AS (
	SELECT nonce
	FROM notification_delivery_callback_replays
	WHERE expires_at <= $1
	ORDER BY expires_at, nonce
	LIMIT $2
)
DELETE FROM notification_delivery_callback_replays replay
USING expired
WHERE replay.nonce = expired.nonce`, now.UTC(), limit)
	if err != nil {
		return 0, err
	}
	return tag.RowsAffected(), nil
}

var _ DeliveryCallbackReplayStore = (*PGRepository)(nil)
var _ DeliveryQueue = (*PGRepository)(nil)
var _ DeliveryRequestRepository = (*PGRepository)(nil)
var _ FeedProjectionQueue = (*PGRepository)(nil)

package feed

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// ErrNotFound is returned when an item lookup misses.
var ErrNotFound = errors.New("notification feed item not found")

const feedColumns = `
	id,
	recipient_id,
	event_type,
	channel,
	title,
	body,
	cta_label,
	cta_href,
	payload,
	actor_id,
	actor_name,
	actor_email,
	actor_avatar,
	seen,
	read,
	archived,
	delivery_status,
	delivered_at,
	seen_at,
	read_at,
	archived_at,
	provider,
	provider_transaction_id,
	source`

type PGRepository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *PGRepository {
	return &PGRepository{pool: pool}
}

// Create inserts a new feed item. On unique-key conflict (same recipient +
// channel + provider transaction id) the existing row is returned. This
// keeps the create path idempotent under Novu webhook retries.
func (r *PGRepository) Create(ctx context.Context, params CreateParams) (*Notification, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("feed repository not configured")
	}
	if strings.TrimSpace(params.ID) == "" || strings.TrimSpace(params.RecipientID) == "" || strings.TrimSpace(params.EventType) == "" {
		return nil, fmt.Errorf("feed create: id, recipient_id, event_type required")
	}

	payload := params.Payload
	if payload == nil {
		payload = map[string]any{}
	}
	payloadJSON, err := json.Marshal(payload)
	if err != nil {
		return nil, fmt.Errorf("encode payload: %w", err)
	}
	channel := strings.TrimSpace(params.Channel)
	if channel == "" {
		channel = ChannelInApp
	}
	provider := strings.TrimSpace(params.Provider)
	if provider == "" {
		provider = "novu"
	}

	row := r.pool.QueryRow(ctx, `
INSERT INTO notification_feed_items (
	id, recipient_id, event_type, channel, title, body, cta_label, cta_href,
	payload, actor_id, actor_name, actor_email, actor_avatar,
	provider, provider_transaction_id, source
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::jsonb, $10, $11, $12, $13, $14, $15, $16)
ON CONFLICT (recipient_id, channel, provider_transaction_id) WHERE provider_transaction_id <> ''
DO UPDATE SET
	-- The upsert keeps the latest dispatch metadata. We don't reset
	-- read/seen — once a user has interacted, a webhook retry shouldn't
	-- "un-read" the item.
	title = EXCLUDED.title,
	body  = EXCLUDED.body,
	cta_label = EXCLUDED.cta_label,
	cta_href  = EXCLUDED.cta_href,
	payload   = EXCLUDED.payload,
	actor_id  = EXCLUDED.actor_id,
	actor_name = EXCLUDED.actor_name,
	actor_email = EXCLUDED.actor_email,
	actor_avatar = EXCLUDED.actor_avatar
RETURNING `+feedColumns,
		params.ID, params.RecipientID, params.EventType, channel,
		params.Title, params.Body, params.CtaLabel, params.CtaHref,
		string(payloadJSON),
		params.ActorID, params.ActorName, params.ActorEmail, params.ActorAvatar,
		provider, params.ProviderTransactionID, params.Source,
	)

	return scanNotification(row)
}

// Get returns a single item.
func (r *PGRepository) Get(ctx context.Context, recipientID, id string) (*Notification, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("feed repository not configured")
	}
	row := r.pool.QueryRow(ctx, `
SELECT `+feedColumns+`
FROM notification_feed_items
WHERE recipient_id = $1 AND id = $2`, recipientID, id)

	return scanNotification(row)
}

// List returns a paginated feed scoped to one recipient.
func (r *PGRepository) List(ctx context.Context, params ListParams) (*Feed, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("feed repository not configured")
	}
	if strings.TrimSpace(params.RecipientID) == "" {
		return nil, fmt.Errorf("feed list: recipient_id required")
	}

	limit := params.Limit
	if limit <= 0 {
		limit = 20
	}
	if limit > 100 {
		limit = 100
	}
	offset := params.Page * limit

	// Build dynamic WHERE clause.
	var (
		clauses []string
		args    []any
	)
	clauses = append(clauses, "recipient_id = $1")
	args = append(args, params.RecipientID)

	archived := false
	if params.Archived != nil {
		archived = *params.Archived
	}
	args = append(args, archived)
	clauses = append(clauses, fmt.Sprintf("archived = $%d", len(args)))

	if params.Read != nil {
		args = append(args, *params.Read)
		clauses = append(clauses, fmt.Sprintf("read = $%d", len(args)))
	}

	whereClause := strings.Join(clauses, " AND ")

	// Total count for pagination.
	var total int
	if err := r.pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM notification_feed_items WHERE `+whereClause,
		args...,
	).Scan(&total); err != nil {
		return nil, fmt.Errorf("feed list count: %w", err)
	}

	args = append(args, limit, offset)
	rows, err := r.pool.Query(ctx, `
SELECT `+feedColumns+`
FROM notification_feed_items
WHERE `+whereClause+`
ORDER BY delivered_at DESC
LIMIT $`+fmt.Sprintf("%d", len(args)-1)+` OFFSET $`+fmt.Sprintf("%d", len(args)),
		args...,
	)
	if err != nil {
		return nil, fmt.Errorf("feed list query: %w", err)
	}
	defer rows.Close()

	items := make([]Notification, 0, limit)
	for rows.Next() {
		n, err := scanNotification(rows)
		if err != nil {
			return nil, err
		}
		items = append(items, *n)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("feed list iterate: %w", err)
	}

	return &Feed{
		Notifications: items,
		TotalCount:    total,
		HasMore:       offset+len(items) < total,
	}, nil
}

// UnreadCount counts unread, non-archived items for the recipient.
func (r *PGRepository) UnreadCount(ctx context.Context, recipientID string) (int, error) {
	if r == nil || r.pool == nil {
		return 0, fmt.Errorf("feed repository not configured")
	}
	var n int
	err := r.pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM notification_feed_items
		 WHERE recipient_id = $1 AND archived = FALSE AND read = FALSE`,
		recipientID,
	).Scan(&n)
	return n, err
}

// UnseenCount counts items the user hasn't yet glanced at.
func (r *PGRepository) UnseenCount(ctx context.Context, recipientID string) (int, error) {
	if r == nil || r.pool == nil {
		return 0, fmt.Errorf("feed repository not configured")
	}
	var n int
	err := r.pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM notification_feed_items
		 WHERE recipient_id = $1 AND archived = FALSE AND seen = FALSE`,
		recipientID,
	).Scan(&n)
	return n, err
}

// MarkRead flips a single item to read=true (idempotent).
func (r *PGRepository) MarkRead(ctx context.Context, recipientID, id string, at time.Time) (*Notification, error) {
	row := r.pool.QueryRow(ctx, `
UPDATE notification_feed_items
SET read = TRUE,
    read_at = COALESCE(read_at, $3),
    seen = TRUE,
    seen_at = COALESCE(seen_at, $3)
WHERE recipient_id = $1 AND id = $2
RETURNING `+feedColumns, recipientID, id, at)
	return scanNotification(row)
}

// MarkAllRead flips every unread item for the recipient. Returns the count.
func (r *PGRepository) MarkAllRead(ctx context.Context, recipientID string, at time.Time) (int, error) {
	tag, err := r.pool.Exec(ctx, `
UPDATE notification_feed_items
SET read = TRUE,
    read_at = COALESCE(read_at, $2),
    seen = TRUE,
    seen_at = COALESCE(seen_at, $2)
WHERE recipient_id = $1 AND archived = FALSE AND read = FALSE`, recipientID, at)
	if err != nil {
		return 0, err
	}
	return int(tag.RowsAffected()), nil
}

// MarkAllSeen flips the seen flag for every visible item (clears the
// "you have N new" badge after the user opens the dropdown).
func (r *PGRepository) MarkAllSeen(ctx context.Context, recipientID string, at time.Time) (int, error) {
	tag, err := r.pool.Exec(ctx, `
UPDATE notification_feed_items
SET seen = TRUE,
    seen_at = COALESCE(seen_at, $2)
WHERE recipient_id = $1 AND archived = FALSE AND seen = FALSE`, recipientID, at)
	if err != nil {
		return 0, err
	}
	return int(tag.RowsAffected()), nil
}

// Archive soft-deletes an item.
func (r *PGRepository) Archive(ctx context.Context, recipientID, id string, at time.Time) error {
	tag, err := r.pool.Exec(ctx, `
UPDATE notification_feed_items
SET archived = TRUE,
    archived_at = COALESCE(archived_at, $3)
WHERE recipient_id = $1 AND id = $2`, recipientID, id, at)
	if err != nil {
		return err
	}
	if tag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}

type scannable interface {
	Scan(dest ...any) error
}

func scanNotification(row scannable) (*Notification, error) {
	var n Notification
	var payloadBytes []byte
	if err := row.Scan(
		&n.ID,
		&n.RecipientID,
		&n.EventType,
		&n.Channel,
		&n.Title,
		&n.Body,
		&n.CtaLabel,
		&n.CtaHref,
		&payloadBytes,
		&n.ActorID,
		&n.ActorName,
		&n.ActorEmail,
		&n.ActorAvatar,
		&n.Seen,
		&n.Read,
		&n.Archived,
		&n.DeliveryStatus,
		&n.DeliveredAt,
		&n.SeenAt,
		&n.ReadAt,
		&n.ArchivedAt,
		&n.Provider,
		&n.ProviderTransactionID,
		&n.Source,
	); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("scan notification: %w", err)
	}
	if len(payloadBytes) > 0 {
		if err := json.Unmarshal(payloadBytes, &n.Payload); err != nil {
			return nil, fmt.Errorf("decode payload: %w", err)
		}
	} else {
		n.Payload = map[string]any{}
	}
	return &n, nil
}

package subscribers

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// ErrNotFound is returned when a lookup by user_id misses.
var ErrNotFound = errors.New("subscriber not found")

const subscriberColumns = `
	user_id,
	novu_subscriber_id,
	email,
	phone,
	first_name,
	last_name,
	avatar,
	locale,
	timezone,
	COALESCE(org_id, ''),
	role,
	created_at,
	updated_at,
	last_synced_at`

type PGRepository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *PGRepository {
	return &PGRepository{pool: pool}
}

func (r *PGRepository) Get(ctx context.Context, userID string) (*Subscriber, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("subscriber repository not configured")
	}
	if strings.TrimSpace(userID) == "" {
		return nil, ErrNotFound
	}

	row := r.pool.QueryRow(ctx, `
SELECT `+subscriberColumns+`
FROM notification_subscribers
WHERE user_id = $1`, userID)

	return scanSubscriber(row)
}

// Upsert merges the params with any existing row. Empty params are preserved
// from the existing row (we never wipe an identity field with "" — only an
// explicit non-empty value overwrites). Returns the post-upsert state.
func (r *PGRepository) Upsert(ctx context.Context, params UpsertParams) (*Subscriber, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("subscriber repository not configured")
	}
	userID := strings.TrimSpace(params.UserID)
	if userID == "" {
		return nil, fmt.Errorf("subscriber upsert: user_id required")
	}
	novuID := strings.TrimSpace(params.NovuSubscriberID)
	if novuID == "" {
		novuID = userID
	}

	row := r.pool.QueryRow(ctx, `
INSERT INTO notification_subscribers (
	user_id, novu_subscriber_id, email, phone, first_name, last_name,
	avatar, locale, timezone, org_id, role, created_at, updated_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULLIF($10, ''), $11, NOW(), NOW())
ON CONFLICT (user_id) DO UPDATE SET
	novu_subscriber_id = COALESCE(NULLIF(EXCLUDED.novu_subscriber_id, ''), notification_subscribers.novu_subscriber_id),
	email              = COALESCE(NULLIF(EXCLUDED.email, ''),              notification_subscribers.email),
	phone              = COALESCE(NULLIF(EXCLUDED.phone, ''),              notification_subscribers.phone),
	first_name         = COALESCE(NULLIF(EXCLUDED.first_name, ''),         notification_subscribers.first_name),
	last_name          = COALESCE(NULLIF(EXCLUDED.last_name, ''),          notification_subscribers.last_name),
	avatar             = COALESCE(NULLIF(EXCLUDED.avatar, ''),             notification_subscribers.avatar),
	locale             = COALESCE(NULLIF(EXCLUDED.locale, ''),             notification_subscribers.locale),
	timezone           = COALESCE(NULLIF(EXCLUDED.timezone, ''),           notification_subscribers.timezone),
	org_id             = COALESCE(EXCLUDED.org_id,                         notification_subscribers.org_id),
	role               = COALESCE(NULLIF(EXCLUDED.role, ''),               notification_subscribers.role),
	updated_at         = NOW()
RETURNING `+subscriberColumns,
		userID, novuID, params.Email, params.Phone, params.FirstName, params.LastName,
		params.Avatar, params.Locale, params.Timezone, params.OrgID, params.Role,
	)

	return scanSubscriber(row)
}

// MarkSynced records when we last pushed this subscriber to Novu.
func (r *PGRepository) MarkSynced(ctx context.Context, userID string, syncedAt time.Time) error {
	if r == nil || r.pool == nil {
		return fmt.Errorf("subscriber repository not configured")
	}
	_, err := r.pool.Exec(ctx,
		`UPDATE notification_subscribers SET last_synced_at = $2 WHERE user_id = $1`,
		userID, syncedAt)
	return err
}

func scanSubscriber(row pgx.Row) (*Subscriber, error) {
	var s Subscriber
	if err := row.Scan(
		&s.UserID,
		&s.NovuSubscriberID,
		&s.Email,
		&s.Phone,
		&s.FirstName,
		&s.LastName,
		&s.Avatar,
		&s.Locale,
		&s.Timezone,
		&s.OrgID,
		&s.Role,
		&s.CreatedAt,
		&s.UpdatedAt,
		&s.LastSyncedAt,
	); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("scan subscriber: %w", err)
	}
	return &s, nil
}

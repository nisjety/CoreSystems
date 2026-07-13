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

const scopedSubscriberColumns = `
	s.user_id,
	m.provider_subscriber_id,
	s.email,
	s.phone,
	s.first_name,
	s.last_name,
	s.avatar,
	s.locale,
	s.timezone,
	COALESCE(m.organization_id, ''),
	m.role,
	s.created_at,
	s.updated_at,
	s.last_synced_at`

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

func (r *PGRepository) GetActiveForOrganization(ctx context.Context, organizationID, userID string) (*Subscriber, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("subscriber repository not configured")
	}
	organizationID = strings.TrimSpace(organizationID)
	userID = strings.TrimSpace(userID)
	if organizationID == "" || userID == "" {
		return nil, ErrNotFound
	}

	row := r.pool.QueryRow(ctx, `
SELECT `+scopedSubscriberColumns+`
FROM notification_subscribers s
JOIN notification_subscriber_memberships m
  ON m.user_id = s.user_id
 AND m.organization_id = $1
 AND m.status = $2
WHERE s.user_id = $3`, organizationID, MembershipStatusActive, userID)

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

func (r *PGRepository) UpsertMembership(ctx context.Context, params MembershipParams) (*Membership, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("subscriber repository not configured")
	}
	organizationID := strings.TrimSpace(params.OrganizationID)
	userID := strings.TrimSpace(params.UserID)
	status := strings.TrimSpace(params.Status)
	if organizationID == "" || userID == "" {
		return nil, fmt.Errorf("membership organization_id and user_id are required")
	}
	if status != MembershipStatusActive && status != MembershipStatusRemoved {
		return nil, fmt.Errorf("membership status is invalid")
	}
	occurredAt := params.OccurredAt.UTC()
	if occurredAt.IsZero() {
		return nil, fmt.Errorf("membership occurred_at is required")
	}

	row := r.pool.QueryRow(ctx, `
INSERT INTO notification_subscriber_memberships (
	organization_id, user_id, provider_subscriber_id, role, status, authority_revision,
	source_event_id, occurred_at, updated_at
) VALUES ($1, $2, $3, $4, $5, $6, NULLIF($7, ''), $8, NOW())
ON CONFLICT (organization_id, user_id) DO UPDATE SET
	provider_subscriber_id = EXCLUDED.provider_subscriber_id,
	role = EXCLUDED.role,
	status = EXCLUDED.status,
	authority_revision = EXCLUDED.authority_revision,
	source_event_id = EXCLUDED.source_event_id,
	occurred_at = EXCLUDED.occurred_at,
	updated_at = NOW()
WHERE
	(EXCLUDED.authority_revision IS NOT NULL AND
	 (notification_subscriber_memberships.authority_revision IS NULL OR
	  EXCLUDED.authority_revision > notification_subscriber_memberships.authority_revision))
	OR
	(EXCLUDED.authority_revision IS NULL AND
	 notification_subscriber_memberships.authority_revision IS NULL AND
	 EXCLUDED.occurred_at > notification_subscriber_memberships.occurred_at)
RETURNING organization_id, user_id, provider_subscriber_id, role, status, authority_revision,
	COALESCE(source_event_id, ''), occurred_at, updated_at`,
		organizationID,
		userID,
		strings.TrimSpace(params.ProviderSubscriberID),
		strings.TrimSpace(params.Role),
		status,
		params.AuthorityRevision,
		strings.TrimSpace(params.SourceEventID),
		occurredAt,
	)

	membership, err := scanMembership(row)
	if errors.Is(err, ErrNotFound) {
		return r.GetMembership(ctx, organizationID, userID)
	}
	return membership, err
}

func (r *PGRepository) GetMembership(ctx context.Context, organizationID, userID string) (*Membership, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("subscriber repository not configured")
	}
	row := r.pool.QueryRow(ctx, `
SELECT organization_id, user_id, provider_subscriber_id, role, status, authority_revision,
	COALESCE(source_event_id, ''), occurred_at, updated_at
FROM notification_subscriber_memberships
WHERE organization_id = $1 AND user_id = $2`, strings.TrimSpace(organizationID), strings.TrimSpace(userID))
	return scanMembership(row)
}

func (r *PGRepository) ListActiveMembershipsForUser(ctx context.Context, userID string) ([]Membership, error) {
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("subscriber repository not configured")
	}
	rows, err := r.pool.Query(ctx, `
SELECT organization_id, user_id, provider_subscriber_id, role, status, authority_revision,
	COALESCE(source_event_id, ''), occurred_at, updated_at
FROM notification_subscriber_memberships
WHERE user_id = $1 AND status = $2
ORDER BY organization_id`, strings.TrimSpace(userID), MembershipStatusActive)
	if err != nil {
		return nil, fmt.Errorf("list active subscriber memberships: %w", err)
	}
	defer rows.Close()

	memberships := make([]Membership, 0)
	for rows.Next() {
		membership, scanErr := scanMembership(rows)
		if scanErr != nil {
			return nil, scanErr
		}
		memberships = append(memberships, *membership)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate active subscriber memberships: %w", err)
	}
	return memberships, nil
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

func scanMembership(row pgx.Row) (*Membership, error) {
	var membership Membership
	if err := row.Scan(
		&membership.OrganizationID,
		&membership.UserID,
		&membership.ProviderSubscriberID,
		&membership.Role,
		&membership.Status,
		&membership.AuthorityRevision,
		&membership.SourceEventID,
		&membership.OccurredAt,
		&membership.UpdatedAt,
	); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("scan subscriber membership: %w", err)
	}
	return &membership, nil
}

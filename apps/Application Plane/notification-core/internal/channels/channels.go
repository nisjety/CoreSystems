// Package channels manages org-level (event_type, channel) configuration:
// is this combo allowed for the org, and is it on by default for new
// subscribers?
//
// The default policy row uses org_id="_default" — when an org has no
// explicit row, this seed is used (loaded in migration 005).
//
// U5-2 (ui-ux-velion-gap.md §10).
package channels

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// DefaultOrgID is the row used as fallback when an org has no explicit
// config. Match the seed value in migration 005.
const DefaultOrgID = "_default"

// Config is one (org, event_type, channel) row. JSON tags match velion's
// `ChannelConfig` type in src/lib/notifications/types.ts.
type Config struct {
	OrgID                 string    `json:"org_id"`
	EventType             string    `json:"event_type"`
	Channel               string    `json:"channel"`
	Enabled               bool      `json:"enabled"`
	DefaultForSubscribers bool      `json:"default_for_subscribers"`
	Label                 string    `json:"label"`
	Description           string    `json:"description"`
	CreatedAt             time.Time `json:"created_at"`
	UpdatedAt             time.Time `json:"updated_at"`
}

// PatchParams is the input to admin PATCH calls.
type PatchParams struct {
	OrgID                 string
	EventType             string
	Channel               string
	Enabled               *bool
	DefaultForSubscribers *bool
	Label                 *string
	Description           *string
}

type Repository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *Repository {
	return &Repository{pool: pool}
}

type Service struct {
	repo *Repository
}

func NewService(repo *Repository) *Service {
	return &Service{repo: repo}
}

// ListForOrg merges org-specific rows with the _default fallback so the
// returned list is the full effective matrix. A nil orgID returns only
// the _default rows (used during onboarding before an org exists).
func (s *Service) ListForOrg(ctx context.Context, orgID string) ([]Config, error) {
	if s == nil {
		return nil, errors.New("channels service not configured")
	}
	return s.repo.ListMerged(ctx, orgID)
}

// Patch upserts a row for a specific org. Pointers in PatchParams are nil
// when the caller wants the existing value preserved.
func (s *Service) Patch(ctx context.Context, params PatchParams) (*Config, error) {
	if s == nil {
		return nil, errors.New("channels service not configured")
	}
	return s.repo.Upsert(ctx, params)
}

func (r *Repository) ListMerged(ctx context.Context, orgID string) ([]Config, error) {
	// We pull both the org-specific and the default rows in one go and
	// merge in Go — Postgres can do this with a WITH/COALESCE but the
	// shape stays clearer here.
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		orgID = DefaultOrgID
	}

	rows, err := r.pool.Query(ctx, `
SELECT org_id, event_type, channel, enabled, default_for_subscribers,
       label, description, created_at, updated_at
FROM notification_channel_configs
WHERE org_id IN ($1, $2)
ORDER BY event_type, channel,
         -- org-specific row first; default last so it's the fallback.
         CASE WHEN org_id = $1 THEN 0 ELSE 1 END`,
		orgID, DefaultOrgID)
	if err != nil {
		return nil, fmt.Errorf("channels list: %w", err)
	}
	defer rows.Close()

	// Map keyed by (event_type, channel). The ORDER BY above guarantees
	// the org-specific row hits the map first, so it wins on duplicates.
	type key struct{ event, channel string }
	seen := make(map[key]Config)
	keys := make([]key, 0)

	for rows.Next() {
		var c Config
		if err := rows.Scan(
			&c.OrgID, &c.EventType, &c.Channel, &c.Enabled, &c.DefaultForSubscribers,
			&c.Label, &c.Description, &c.CreatedAt, &c.UpdatedAt,
		); err != nil {
			return nil, fmt.Errorf("scan channel config: %w", err)
		}
		k := key{event: c.EventType, channel: c.Channel}
		if _, exists := seen[k]; !exists {
			seen[k] = c
			keys = append(keys, k)
		}
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}

	out := make([]Config, 0, len(keys))
	for _, k := range keys {
		c := seen[k]
		// Surface the user-facing org id even when the underlying row is
		// the default fallback.
		if c.OrgID == DefaultOrgID {
			c.OrgID = orgID
		}
		out = append(out, c)
	}
	return out, nil
}

func (r *Repository) Upsert(ctx context.Context, params PatchParams) (*Config, error) {
	orgID := strings.TrimSpace(params.OrgID)
	eventType := strings.TrimSpace(params.EventType)
	channel := strings.TrimSpace(params.Channel)
	if orgID == "" || eventType == "" || channel == "" {
		return nil, errors.New("org_id, event_type, channel required")
	}

	// Pull existing or default to fill the gaps.
	existing, _ := r.fetchOne(ctx, orgID, eventType, channel)
	if existing == nil {
		existing, _ = r.fetchOne(ctx, DefaultOrgID, eventType, channel)
	}

	var enabled, defaultForSubs bool
	var label, description string
	if existing != nil {
		enabled = existing.Enabled
		defaultForSubs = existing.DefaultForSubscribers
		label = existing.Label
		description = existing.Description
	}
	if params.Enabled != nil {
		enabled = *params.Enabled
	}
	if params.DefaultForSubscribers != nil {
		defaultForSubs = *params.DefaultForSubscribers
	}
	if params.Label != nil {
		label = *params.Label
	}
	if params.Description != nil {
		description = *params.Description
	}

	row := r.pool.QueryRow(ctx, `
INSERT INTO notification_channel_configs
  (org_id, event_type, channel, enabled, default_for_subscribers, label, description, created_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())
ON CONFLICT (org_id, event_type, channel) DO UPDATE SET
	enabled                 = EXCLUDED.enabled,
	default_for_subscribers = EXCLUDED.default_for_subscribers,
	label                   = EXCLUDED.label,
	description             = EXCLUDED.description,
	updated_at              = NOW()
RETURNING org_id, event_type, channel, enabled, default_for_subscribers,
          label, description, created_at, updated_at`,
		orgID, eventType, channel, enabled, defaultForSubs, label, description)

	var c Config
	if err := row.Scan(
		&c.OrgID, &c.EventType, &c.Channel, &c.Enabled, &c.DefaultForSubscribers,
		&c.Label, &c.Description, &c.CreatedAt, &c.UpdatedAt,
	); err != nil {
		return nil, fmt.Errorf("upsert channel config: %w", err)
	}
	return &c, nil
}

func (r *Repository) fetchOne(ctx context.Context, orgID, eventType, channel string) (*Config, error) {
	row := r.pool.QueryRow(ctx, `
SELECT org_id, event_type, channel, enabled, default_for_subscribers,
       label, description, created_at, updated_at
FROM notification_channel_configs
WHERE org_id = $1 AND event_type = $2 AND channel = $3`,
		orgID, eventType, channel)

	var c Config
	if err := row.Scan(
		&c.OrgID, &c.EventType, &c.Channel, &c.Enabled, &c.DefaultForSubscribers,
		&c.Label, &c.Description, &c.CreatedAt, &c.UpdatedAt,
	); err != nil {
		return nil, err
	}
	return &c, nil
}

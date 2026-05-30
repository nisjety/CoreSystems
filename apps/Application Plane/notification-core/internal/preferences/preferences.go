// Package preferences manages per-user, per-(event_type, channel) opt-in
// state. Combined with `channels` (org-level defaults), these resolve to
// the effective preference matrix served to `/profile/notifications`.
//
// Writes also fan out to Novu via PATCH /v1/subscribers/{id}/preferences/
// {workflowId} so cross-channel delivery respects the user's choice.
//
// U5-2 (ui-ux-velion-gap.md §10).
package preferences

import (
	"context"
	"errors"
	"fmt"
	"log"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// ErrNotFound is returned when a Get misses.
var ErrNotFound = errors.New("preference not found")

// Preference is one row in `notification_preferences`. JSON tags match
// velion's `Preference` type in src/lib/notifications/types.ts.
type Preference struct {
	UserID    string    `json:"user_id"`
	EventType string    `json:"event_type"`
	Channel   string    `json:"channel"`
	Enabled   bool      `json:"enabled"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

// PutParams is the input to Service.Put.
type PutParams struct {
	UserID    string
	EventType string
	Channel   string
	Enabled   bool
}

// NovuPreferenceClient is the runtime hook for syncing user preferences
// to Novu. Errors are logged but don't fail the local write.
type NovuPreferenceClient interface {
	UpdateSubscriberPreference(ctx context.Context, subscriberID, workflowID, channel string, enabled bool) error
}

type Repository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *Repository {
	return &Repository{pool: pool}
}

type Service struct {
	repo    *Repository
	runtime NovuPreferenceClient
}

func NewService(repo *Repository, runtime NovuPreferenceClient) *Service {
	return &Service{repo: repo, runtime: runtime}
}

// ListForUser returns every explicit preference row for one user. Combined
// with `channels.Config` defaults in the HTTP handler this becomes the
// effective matrix shown in the UI.
func (s *Service) ListForUser(ctx context.Context, userID string) ([]Preference, error) {
	if s == nil {
		return nil, errors.New("preferences service not configured")
	}
	return s.repo.ListForUser(ctx, userID)
}

// Put upserts a single (user, event_type, channel) preference. Best-effort
// Novu sync runs in a detached goroutine — same contract as the subscriber
// sync — so the HTTP write returns immediately.
func (s *Service) Put(ctx context.Context, params PutParams) (*Preference, error) {
	if s == nil {
		return nil, errors.New("preferences service not configured")
	}

	pref, err := s.repo.Upsert(ctx, params)
	if err != nil {
		return nil, err
	}

	if s.runtime != nil {
		go func(p Preference) {
			rctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			if err := s.runtime.UpdateSubscriberPreference(rctx, p.UserID, p.EventType, p.Channel, p.Enabled); err != nil {
				log.Printf("[notification-core/preferences] Novu sync failed for %s/%s/%s: %v", p.UserID, p.EventType, p.Channel, err)
			}
		}(*pref)
	}

	return pref, nil
}

// IsEnabled returns the effective enabled state for one (user, event_type,
// channel). If no preference row exists, returns the fallback value
// supplied by the caller (typically the org-level default).
func (s *Service) IsEnabled(ctx context.Context, userID, eventType, channel string, fallback bool) (bool, error) {
	if s == nil {
		return fallback, errors.New("preferences service not configured")
	}
	pref, err := s.repo.Get(ctx, userID, eventType, channel)
	if errors.Is(err, ErrNotFound) {
		return fallback, nil
	}
	if err != nil {
		return fallback, err
	}
	return pref.Enabled, nil
}

func (r *Repository) ListForUser(ctx context.Context, userID string) ([]Preference, error) {
	if strings.TrimSpace(userID) == "" {
		return nil, errors.New("user_id required")
	}
	rows, err := r.pool.Query(ctx, `
SELECT user_id, event_type, channel, enabled, created_at, updated_at
FROM notification_preferences
WHERE user_id = $1
ORDER BY event_type, channel`, userID)
	if err != nil {
		return nil, fmt.Errorf("list preferences: %w", err)
	}
	defer rows.Close()

	out := make([]Preference, 0)
	for rows.Next() {
		var p Preference
		if err := rows.Scan(&p.UserID, &p.EventType, &p.Channel, &p.Enabled, &p.CreatedAt, &p.UpdatedAt); err != nil {
			return nil, fmt.Errorf("scan preference: %w", err)
		}
		out = append(out, p)
	}
	return out, rows.Err()
}

func (r *Repository) Get(ctx context.Context, userID, eventType, channel string) (*Preference, error) {
	row := r.pool.QueryRow(ctx, `
SELECT user_id, event_type, channel, enabled, created_at, updated_at
FROM notification_preferences
WHERE user_id = $1 AND event_type = $2 AND channel = $3`, userID, eventType, channel)
	var p Preference
	if err := row.Scan(&p.UserID, &p.EventType, &p.Channel, &p.Enabled, &p.CreatedAt, &p.UpdatedAt); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("get preference: %w", err)
	}
	return &p, nil
}

func (r *Repository) Upsert(ctx context.Context, params PutParams) (*Preference, error) {
	userID := strings.TrimSpace(params.UserID)
	eventType := strings.TrimSpace(params.EventType)
	channel := strings.TrimSpace(params.Channel)
	if userID == "" || eventType == "" || channel == "" {
		return nil, errors.New("user_id, event_type and channel are required")
	}

	row := r.pool.QueryRow(ctx, `
INSERT INTO notification_preferences (user_id, event_type, channel, enabled, created_at, updated_at)
VALUES ($1, $2, $3, $4, NOW(), NOW())
ON CONFLICT (user_id, event_type, channel) DO UPDATE SET
	enabled    = EXCLUDED.enabled,
	updated_at = NOW()
RETURNING user_id, event_type, channel, enabled, created_at, updated_at`,
		userID, eventType, channel, params.Enabled)

	var p Preference
	if err := row.Scan(&p.UserID, &p.EventType, &p.Channel, &p.Enabled, &p.CreatedAt, &p.UpdatedAt); err != nil {
		return nil, fmt.Errorf("upsert preference: %w", err)
	}
	return &p, nil
}

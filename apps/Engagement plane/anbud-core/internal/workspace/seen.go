package workspace

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
)

// Seen is when the organisation last marked its tenders as seen, and whether it ever has.
func (s *Store) Seen(ctx context.Context, org string) (time.Time, bool, error) {
	var at time.Time
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		return tx.QueryRow(ctx, `SELECT seen_at FROM tenant.workspace_seen WHERE org_id = $1`, org).Scan(&at)
	})
	if errors.Is(err, pgx.ErrNoRows) {
		return time.Time{}, false, nil
	}
	if err != nil {
		return time.Time{}, false, fmt.Errorf("workspace: read seen: %w", err)
	}
	return at, true, nil
}

// MarkSeen records that the organisation has now looked at its tenders, and returns the instant it recorded.
func (s *Store) MarkSeen(ctx context.Context, org, by string) (time.Time, error) {
	if !validActor(by) {
		return time.Time{}, fmt.Errorf("%w: the user id has an invalid shape", ErrInvalid)
	}
	now := s.now().UTC()
	err := s.scoped(ctx, org, func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx, `
INSERT INTO tenant.workspace_seen (org_id, seen_at, updated_by)
VALUES ($1, $2, $3)
ON CONFLICT (org_id) DO UPDATE SET seen_at = EXCLUDED.seen_at, updated_by = EXCLUDED.updated_by`, org, now, by)
		return err
	})
	if err != nil {
		return time.Time{}, fmt.Errorf("workspace: mark seen: %w", err)
	}
	return now, nil
}

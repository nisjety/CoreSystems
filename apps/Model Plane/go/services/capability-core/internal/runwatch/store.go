package runwatch

import (
	"context"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// PostgresSubscriptionStore is the pgx-backed SubscriptionStore over
// run_watch_subscriptions (migrations/0012_run_watch_subscriptions.up.sql), and
// the ScheduleOwnerChecker over cron_schedules. Its queries are deliberately
// simple and are not separately unit-tested (no live database in this package's
// tests) — the same convention taskexec.RunCompletionConsumer.closeTask follows
// for its own direct pool queries; [Notifier.process] is what carries the
// unit-tested decision logic, over the [SubscriptionStore] and
// [ScheduleOwnerChecker] interfaces this satisfies. The owner predicate is
// pinned on a real Postgres by store_integration_test.go (build tag integration).
type PostgresSubscriptionStore struct {
	pool *pgxpool.Pool
}

// NewPostgresSubscriptionStore constructs the store.
func NewPostgresSubscriptionStore(pool *pgxpool.Pool) *PostgresSubscriptionStore {
	return &PostgresSubscriptionStore{pool: pool}
}

// PendingWatchers returns every still-pending, non-deleted watcher for
// (orgID, runID), backed by run_watch_subscriptions_pending_idx.
func (s *PostgresSubscriptionStore) PendingWatchers(ctx context.Context, orgID, runID string) ([]Watcher, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT id, user_id
		FROM run_watch_subscriptions
		WHERE org_id = $1 AND run_id = $2 AND status = 'pending' AND deleted_at IS NULL
	`, orgID, runID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var watchers []Watcher
	for rows.Next() {
		var w Watcher
		if err := rows.Scan(&w.ID, &w.UserID); err != nil {
			return nil, err
		}
		watchers = append(watchers, w)
	}
	return watchers, rows.Err()
}

// MarkNotified flips one watcher row to 'notified'. The status='pending'
// predicate is the redelivery guard: a row already flipped by a prior
// delivery of the same terminal event matches zero rows here, so a
// redelivered message can never notify the same watcher twice through this
// path (see the package doc's manual-ack design note).
func (s *PostgresSubscriptionStore) MarkNotified(ctx context.Context, watcherID, eventType string, notifiedAt time.Time) error {
	_, err := s.pool.Exec(ctx, `
		UPDATE run_watch_subscriptions
		SET status = 'notified', event_type = $1, notified_at = $2, updated_at = $2
		WHERE id = $3 AND status = 'pending' AND deleted_at IS NULL
	`, eventType, notifiedAt, watcherID)
	return err
}

// ScheduleCreatedBy reports whether scheduleID is a live (not deleted) schedule of orgID whose creator is
// subjectID. It is the predicate cron.RunNow applies to a creator's own run-now press, read from
// capability-core's own cron_schedules table, so a scheduled-outcome notice reaches only the creator
// (F-f05-06). A schedule of another org, a schedule of another creator and a deleted schedule are all false.
func (s *PostgresSubscriptionStore) ScheduleCreatedBy(ctx context.Context, orgID, scheduleID, subjectID string) (bool, error) {
	var created bool
	err := s.pool.QueryRow(ctx, `
		SELECT EXISTS (
			SELECT 1 FROM cron_schedules
			WHERE id = $1 AND org_id = $2 AND creator_subject_id = $3 AND deleted_at IS NULL
		)
	`, scheduleID, orgID, subjectID).Scan(&created)
	if err != nil {
		return false, err
	}
	return created, nil
}

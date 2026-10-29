package rawstore

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
)

const recoverRuns = `
UPDATE sync_runs SET state = 'interrupted', finished_at = now()
WHERE state = 'running'`

const recoverDays = `
UPDATE sync_days SET state = 'interrupted', error_code = 'process_lost', updated_at = now()
WHERE state = 'running'`

// lastRetryAfter reads the retry instant of the latest run that stored one. It
// reads the instant as stored and compares it with nothing.
const lastRetryAfter = `
SELECT retry_after_until
FROM sync_runs
WHERE retry_after_until IS NOT NULL
ORDER BY id DESC
LIMIT 1`

// RecoverStale marks running runs interrupted and running days interrupted with
// error code process_lost. It returns the number of runs marked. Call it only
// while holding the budget lock: a running row then belongs to a dead process.
// Finished runs and finished days are never touched.
func (s *Store) RecoverStale(ctx context.Context) (int64, error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return 0, fmt.Errorf("rawstore: begin recovery: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	tag, err := tx.Exec(ctx, recoverRuns)
	if err != nil {
		return 0, fmt.Errorf("rawstore: mark runs interrupted: %w", err)
	}
	if _, err := tx.Exec(ctx, recoverDays); err != nil {
		return 0, fmt.Errorf("rawstore: mark days interrupted: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return 0, fmt.Errorf("rawstore: commit recovery: %w", err)
	}
	return tag.RowsAffected(), nil
}

// LastRetryAfter returns the retry instant stored by the latest run that stored
// one, exactly as it was stored, and reports whether such a run exists. The store
// makes no comparison with any clock. The engine writes instants with its own
// clock, and the caller compares the instant with its own clock before it calls
// Doffin again.
func (s *Store) LastRetryAfter(ctx context.Context) (time.Time, bool, error) {
	var until time.Time
	err := s.pool.QueryRow(ctx, lastRetryAfter).Scan(&until)
	if errors.Is(err, pgx.ErrNoRows) {
		return time.Time{}, false, nil
	}
	if err != nil {
		return time.Time{}, false, fmt.Errorf("rawstore: read retry instant: %w", err)
	}
	return until.UTC(), true, nil
}

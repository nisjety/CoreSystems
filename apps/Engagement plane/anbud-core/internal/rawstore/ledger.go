package rawstore

import (
	"context"
	"fmt"
	"time"

	"github.com/triodelab/anbud/internal/doffinsync"
)

const insertRun = `
INSERT INTO sync_runs (mode, range_from, range_to)
VALUES ($1, $2::date, $3::date)
RETURNING id`

// finishRun ends a run that is still running. A run that has already ended, or
// that recovery marked interrupted, matches no row.
const finishRun = `
UPDATE sync_runs
SET state = $2, finished_at = now(), retry_after_until = $3, last_error = $4,
    searches = $5, downloads = $6, stored = $7, duplicates = $8, held_skipped = $9, not_found = $10
WHERE id = $1 AND state = 'running'`

// beginDay starts a day for a run that is still running. For any other run the
// SELECT yields no row, so nothing is inserted and nothing is updated.
const beginDay = `
INSERT INTO sync_days (source, day, state, attempts, last_run_id)
SELECT $1::text, $2::date, 'running', 1, $3::bigint
WHERE EXISTS (SELECT 1 FROM sync_runs WHERE id = $3::bigint AND state = 'running')
ON CONFLICT (source, day) DO UPDATE
SET state = 'running', error_code = NULL, num_total = NULL, num_accessible = NULL,
    held = 0, stored = 0, not_found = 0, rejected = 0, unresolved = 0,
    attempts = sync_days.attempts + 1, last_run_id = EXCLUDED.last_run_id, updated_at = now()`

// finishDay writes a day only while the day is running, the run that began it is
// still running, and that run is the one the caller names ($12). A day that recovery
// interrupted, or a run that has ended, keeps the state it was left in.
const finishDay = `
UPDATE sync_days
SET state = $3, error_code = $4, num_total = $5, num_accessible = $6,
    held = $7, stored = $8, not_found = $9, rejected = $10, unresolved = $11, updated_at = now()
WHERE source = $1 AND day = $2::date AND state = 'running'
  AND last_run_id = $12::bigint
  AND EXISTS (SELECT 1 FROM sync_runs AS r WHERE r.id = $12::bigint AND r.state = 'running')`

const selectDayStates = `
SELECT to_char(day, 'YYYY-MM-DD'), state FROM sync_days
WHERE source = $1 AND day BETWEEN $2::date AND $3::date`

// finalReasons are the run outcomes a run may finish with; they are the
// sync_runs.state values that are not running or interrupted.
var finalReasons = map[doffinsync.Reason]bool{
	doffinsync.ReasonDone:        true,
	doffinsync.ReasonPartial:     true,
	doffinsync.ReasonPaused:      true,
	doffinsync.ReasonKeyRejected: true,
	doffinsync.ReasonCancelled:   true,
	doffinsync.ReasonFailed:      true,
}

// BeginRun inserts a sync_runs row in state running and returns its id.
func (s *Store) BeginRun(ctx context.Context, r doffinsync.RunStart) (int64, error) {
	if r.Mode != doffinsync.ModeSync && r.Mode != doffinsync.ModeCensus {
		return 0, fmt.Errorf("%w: mode must be sync or census", ErrInvalidRecord)
	}
	var id int64
	if err := s.pool.QueryRow(ctx, insertRun, string(r.Mode), dateLabel(r.From), dateLabel(r.To)).Scan(&id); err != nil {
		return 0, fmt.Errorf("rawstore: begin run: %w", err)
	}
	return id, nil
}

// FinishRun writes the final state, the counters and the last error of a running
// run. The counters are written here only; per-day state is written as each day ends.
// A run that is no longer running cannot finish again: the error wraps ErrStaleWrite.
func (s *Store) FinishRun(ctx context.Context, runID int64, out doffinsync.RunOutcome) error {
	if !finalReasons[out.Reason] {
		return fmt.Errorf("%w: reason %q does not end a run", ErrInvalidRecord, out.Reason)
	}
	var until *time.Time
	if out.RetryAfterUntil != nil {
		u := out.RetryAfterUntil.UTC()
		until = &u
	}
	var lastErr *string
	if e := cleanText(out.LastError, maxLastError); e != "" {
		lastErr = &e
	}
	c := out.Counters
	tag, err := s.pool.Exec(ctx, finishRun,
		runID, string(out.Reason), until, lastErr,
		c.Searches, c.Downloads, c.Stored, c.Duplicates, c.HeldSkipped, c.NotFound)
	if err != nil {
		return fmt.Errorf("rawstore: finish run %d: %w", runID, err)
	}
	if tag.RowsAffected() != 1 {
		return fmt.Errorf("%w: run %d is not running, so it cannot finish", ErrStaleWrite, runID)
	}
	return nil
}

// BeginDay upserts the sync_days row for day as running and counts one more
// attempt. The counts (held, stored, not found, rejected and unresolved) and the
// totals reset. No other row is touched. A run that is no
// longer running cannot begin a day: the error wraps ErrStaleWrite and nothing is written.
func (s *Store) BeginDay(ctx context.Context, day time.Time, runID int64) error {
	label := dateLabel(day)
	tag, err := s.pool.Exec(ctx, beginDay, s.source, label, runID)
	if err != nil {
		return fmt.Errorf("rawstore: begin day %s: %w", label, err)
	}
	if tag.RowsAffected() != 1 {
		return fmt.Errorf("%w: day %s cannot begin under run %d", ErrStaleWrite, label, runID)
	}
	return nil
}

// FinishDay writes the state and counts of a day that BeginDay started. Empty
// values are written as NULL. A record with no run id is refused before any SQL,
// because it cannot name the run it belongs to. A day that is no longer running,
// whose run has ended, or that another run began, is not written: the error wraps
// ErrStaleWrite and the row stays as it was.
func (s *Store) FinishDay(ctx context.Context, rec doffinsync.DayRecord) error {
	label := dateLabel(rec.Day)
	if rec.RunID == 0 {
		return fmt.Errorf("%w: day %s has no run id", ErrInvalidRecord, label)
	}
	var code *string
	if rec.ErrorCode != "" {
		c := string(rec.ErrorCode)
		code = &c
	}
	tag, err := s.pool.Exec(ctx, finishDay,
		s.source, label, string(rec.State), code,
		rec.Total, rec.Accessible, rec.Held, rec.Stored, rec.NotFound, rec.Rejected, rec.Unresolved, rec.RunID)
	if err != nil {
		return fmt.Errorf("rawstore: finish day %s: %w", label, err)
	}
	if tag.RowsAffected() != 1 {
		return fmt.Errorf("%w: day %s is not running under a running run, so it cannot finish", ErrStaleWrite, label)
	}
	return nil
}

// DayStates returns the state of every day in [from, to] that has a row, keyed
// by its "2006-01-02" label.
func (s *Store) DayStates(ctx context.Context, from, to time.Time) (map[string]doffinsync.DayState, error) {
	rows, err := s.pool.Query(ctx, selectDayStates, s.source, dateLabel(from), dateLabel(to))
	if err != nil {
		return nil, fmt.Errorf("rawstore: read day states: %w", err)
	}
	defer rows.Close()
	out := make(map[string]doffinsync.DayState)
	for rows.Next() {
		var day, state string
		if err := rows.Scan(&day, &state); err != nil {
			return nil, fmt.Errorf("rawstore: scan day state: %w", err)
		}
		out[day] = doffinsync.DayState(state)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("rawstore: iterate day states: %w", err)
	}
	return out, nil
}

// dateLabel is the UTC calendar date of t, as the database's date type takes it.
func dateLabel(t time.Time) string {
	return t.UTC().Format(dayLayout)
}

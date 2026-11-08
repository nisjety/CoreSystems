package pg

// Job control writes (WS15.L2) on the Postgres store. The rules are in
// store.PlanControl; this file only moves the rows. SetControl runs in one
// transaction with the row locked, so two requests for one job are ordered.

import (
	"context"
	"errors"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// SetControl implements store.JobsStore.
func (s *jobsStore) SetControl(orgID string, id quarrycontracts.ID, action string) (store.Job, error) {
	ctx := context.Background()
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return store.Job{}, mapPgErr(err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	j, err := scanJob(tx.QueryRow(ctx,
		`SELECT `+jobColumns+` FROM jobs WHERE id=$1 AND org_id=$2 FOR UPDATE`,
		string(id), orgID))
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return store.Job{}, store.ErrNotFound
		}
		return store.Job{}, mapPgErr(err)
	}
	now := time.Now()
	plan, err := store.PlanControl(j, action)
	if err != nil {
		return store.Job{}, err
	}
	if plan.NoOp {
		return j, nil
	}
	j = store.ApplyControl(j, plan, now)
	if plan.EndNow {
		_, err = tx.Exec(ctx,
			`UPDATE jobs SET status=$2, terminal_reason=$3, finished_at=$4, desired_state=$5, desired_seq=$6, applied_seq=$7 WHERE id=$1`,
			string(id), j.Status, j.TerminalReason, *j.FinishedAt, j.DesiredState, j.DesiredSeq, j.AppliedSeq)
	} else {
		_, err = tx.Exec(ctx,
			`UPDATE jobs SET desired_state=$2, desired_seq=$3 WHERE id=$1`,
			string(id), j.DesiredState, j.DesiredSeq)
	}
	if err != nil {
		return store.Job{}, mapPgErr(err)
	}
	if err := tx.Commit(ctx); err != nil {
		return store.Job{}, mapPgErr(err)
	}
	return j, nil
}

// AckControl implements store.JobsStore. It raises applied_seq and never lowers it.
func (s *jobsStore) AckControl(id quarrycontracts.ID, appliedSeq int64) error {
	tag, err := s.pool.Exec(context.Background(),
		`UPDATE jobs SET applied_seq = GREATEST(applied_seq, $2) WHERE id=$1`,
		string(id), appliedSeq)
	if err != nil {
		return mapPgErr(err)
	}
	if tag.RowsAffected() == 0 {
		return store.ErrNotFound
	}
	return nil
}

// PendingControl implements store.JobsStore. It reads the partial index
// idx_jobs_pending_control (migration 016).
func (s *jobsStore) PendingControl(limit int) []store.Job {
	limit = pageLimit(limit, defaultMaxPage)
	rows, err := s.pool.Query(context.Background(),
		`SELECT `+jobColumns+` FROM jobs WHERE desired_seq > applied_seq
		  ORDER BY created_at ASC, id ASC LIMIT $1`, limit)
	if err != nil {
		return nil
	}
	defer rows.Close()
	out := []store.Job{}
	for rows.Next() {
		j, err := scanJob(rows)
		if err != nil {
			return nil
		}
		out = append(out, j)
	}
	return out
}

// ListRunning implements store.JobsStore.
func (s *jobsStore) ListRunning(limit int) []store.Job {
	limit = pageLimit(limit, defaultMaxPage)
	rows, err := s.pool.Query(context.Background(),
		`SELECT `+jobColumns+` FROM jobs
		  WHERE status IN ('running', 'paused') AND workflow_id <> ''
		  ORDER BY created_at ASC, id ASC LIMIT $1`, limit)
	if err != nil {
		return nil
	}
	defer rows.Close()
	out := []store.Job{}
	for rows.Next() {
		j, err := scanJob(rows)
		if err != nil {
			return nil
		}
		out = append(out, j)
	}
	return out
}

// Finalize implements store.JobsStore. The update only touches a job that has
// not ended, so a second call changes nothing and reports changed=false.
func (s *jobsStore) Finalize(id quarrycontracts.ID, status, reason string, finishedAt int64) (store.Job, bool, error) {
	j, err := scanJob(s.pool.QueryRow(context.Background(),
		`UPDATE jobs SET status=$2, terminal_reason=$3, finished_at=COALESCE(finished_at, $4)
		  WHERE id=$1 AND status NOT IN ('completed', 'failed', 'cancelled', 'timed_out', 'skipped')
		  RETURNING `+jobColumns,
		string(id), status, reason, finishedAt))
	if err == nil {
		return j, true, nil
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return store.Job{}, false, mapPgErr(err)
	}
	cur, ok := s.Get(id)
	if !ok {
		return store.Job{}, false, store.ErrNotFound
	}
	return cur, false, nil
}

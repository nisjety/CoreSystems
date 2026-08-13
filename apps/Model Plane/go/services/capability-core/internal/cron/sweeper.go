package cron

import (
	"bytes"
	"context"
	"encoding/json"
	"log/slog"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Sweeper fires due cron_schedules. It computes each schedule's next fire from a
// cron expression, creates a task from the schedule's task_template, records the
// fire in cron_fires, and advances next_fire_at — all inside one transaction
// using FOR UPDATE SKIP LOCKED so multiple replicas never double-fire.
type Sweeper struct {
	pool     *pgxpool.Pool
	interval time.Duration
}

// NewSweeper constructs a sweeper that ticks every minute.
func NewSweeper(pool *pgxpool.Pool) *Sweeper {
	return &Sweeper{pool: pool, interval: time.Minute}
}

// Start runs the sweep loop until ctx is cancelled. Errors are logged, not fatal.
func (s *Sweeper) Start(ctx context.Context) {
	ticker := time.NewTicker(s.interval)
	defer ticker.Stop()
	s.sweepAndLog(ctx)
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			s.sweepAndLog(ctx)
		}
	}
}

func (s *Sweeper) sweepAndLog(ctx context.Context) {
	fired, err := s.RunOnce(ctx)
	if err != nil {
		slog.Warn("cron sweep failed", "error", err)
		return
	}
	if fired > 0 {
		slog.Info("cron sweep fired schedules", "count", fired)
	}
}

type dueSchedule struct {
	id       string
	orgID    string
	expr     string
	tz       string
	template json.RawMessage
	nextFire *time.Time
}

// RunOnce scans due schedules and fires each exactly once. Returns the count of
// schedules that fired a task (schedules being initialized for the first time,
// or whose expression is invalid, do not count).
func (s *Sweeper) RunOnce(ctx context.Context) (int, error) {
	now := time.Now().UTC()
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return 0, err
	}
	defer func() { _ = tx.Rollback(ctx) }()

	rows, err := tx.Query(ctx, `
		SELECT id, org_id, schedule_expr, timezone, task_template, next_fire_at
		FROM cron_schedules
		WHERE enabled AND deleted_at IS NULL
		  AND (next_fire_at IS NULL OR next_fire_at <= $1)
		ORDER BY next_fire_at NULLS FIRST
		FOR UPDATE SKIP LOCKED
		LIMIT 100
	`, now)
	if err != nil {
		return 0, err
	}
	var dues []dueSchedule
	for rows.Next() {
		var d dueSchedule
		if err := rows.Scan(&d.id, &d.orgID, &d.expr, &d.tz, &d.template, &d.nextFire); err != nil {
			rows.Close()
			return 0, err
		}
		dues = append(dues, d)
	}
	rows.Close()

	fired := 0
	for _, d := range dues {
		next, nerr := NextFrom(d.expr, d.tz, now)
		if nerr != nil {
			// A malformed expression can never fire; disable it so the sweep does
			// not re-select it every tick.
			slog.Warn("disabling cron schedule with invalid expression",
				"id", d.id, "expr", d.expr, "error", nerr)
			_, _ = tx.Exec(ctx, `UPDATE cron_schedules SET enabled=false, updated_at=$1 WHERE id=$2`, now, d.id)
			continue
		}
		if d.nextFire == nil {
			// First observation: initialize next_fire_at only; do not fire on the
			// tick that discovers the schedule.
			_, _ = tx.Exec(ctx, `UPDATE cron_schedules SET next_fire_at=$1, updated_at=$2 WHERE id=$3`, next, now, d.id)
			continue
		}
		s.fireOne(ctx, tx, d, now)
		_, _ = tx.Exec(ctx,
			`UPDATE cron_schedules SET last_fire_at=$1, next_fire_at=$2, updated_at=$1 WHERE id=$3`,
			now, next, d.id)
		fired++
	}
	if err := tx.Commit(ctx); err != nil {
		return 0, err
	}
	return fired, nil
}

// taskConfigJSON carries the schedule's template into the created task's
// config_json.
//
// This is load-bearing, not bookkeeping. taskexec's WorkflowDispatcher reads
// tasks.config_json to decide which workflow a fired task starts
// (taskTemplate.workflow_type / workflow_input / policy in
// internal/taskexec/workflow_dispatcher.go). Until this column was populated,
// the sweeper wrote the template's title/description/assignee/priority into
// their own columns and dropped everything else on the floor, so
// dispatchPlan always fell through to DefaultWorkflowType — a cron schedule
// could not select a workflow no matter what the API or UI stored on it, and
// six of orchestrator-core's seven allowlisted workflows were unreachable
// from a schedule.
//
// The template is passed through VERBATIM rather than as a hand-picked subset:
// dispatchPlan uses a plain json.Unmarshal (unknown fields ignored), so
// forwarding everything means a new dispatch field starts working without a
// matching change here. A hand-picked list is exactly the kind of lockstep
// coupling that let this drift go unnoticed.
//
// An absent or non-object template yields `{}` so the NOT NULL column keeps a
// valid JSON object.
func taskConfigJSON(template []byte) []byte {
	trimmed := bytes.TrimSpace(template)
	if len(trimmed) == 0 {
		return []byte("{}")
	}
	// Only a JSON object is a usable template; an array or scalar would make
	// dispatchPlan's unmarshal fail and turn every fire of this schedule into a
	// failed task.
	var probe map[string]json.RawMessage
	if err := json.Unmarshal(trimmed, &probe); err != nil {
		return []byte("{}")
	}
	return trimmed
}

// fireOne creates a task from the schedule's template and records a cron_fires
// row. Failures are recorded on the fire row (status=failed) rather than
// aborting the whole sweep.
func (s *Sweeper) fireOne(ctx context.Context, tx pgx.Tx, d dueSchedule, now time.Time) {
	var tpl struct {
		Kind        string `json:"kind"`
		Title       string `json:"title"`
		Description string `json:"description"`
		Assignee    string `json:"assignee"`
		Priority    int    `json:"priority"`
	}
	_ = json.Unmarshal(d.template, &tpl)
	if tpl.Kind == "" {
		tpl.Kind = "cron"
	}
	if tpl.Title == "" {
		tpl.Title = "Scheduled task"
	}
	taskID := "task_" + uuid.New().String()
	fireID := "cronfire_" + uuid.New().String()
	_, terr := tx.Exec(ctx, `
		INSERT INTO tasks (id, org_id, kind, title, description, assignee, status,
		    priority, config_json, scheduled_at, created_at, updated_at)
		VALUES ($1,$2,$3,$4,$5,$6,'created',$7,$8,$9,$9,$9)
	`, taskID, d.orgID, tpl.Kind, tpl.Title, tpl.Description, tpl.Assignee, tpl.Priority,
		taskConfigJSON(d.template), now)
	if terr != nil {
		slog.Warn("cron task creation failed", "schedule", d.id, "error", terr)
		_, _ = tx.Exec(ctx,
			`INSERT INTO cron_fires (id, schedule_id, fired_at, status, error) VALUES ($1,$2,$3,'failed',$4)`,
			fireID, d.id, now, terr.Error())
		return
	}
	_, _ = tx.Exec(ctx,
		`INSERT INTO cron_fires (id, schedule_id, task_id, fired_at, status) VALUES ($1,$2,$3,$4,'pending')`,
		fireID, d.id, taskID, now)
}

package cron

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Sweeper fires due cron_schedules. It computes each schedule's next fire from a
// cron expression, creates a task from the schedule's task_template, records the
// fire in cron_fires, and advances next_fire_at — all inside one transaction
// using FOR UPDATE SKIP LOCKED so multiple replicas never double-fire, and each
// schedule runs in its own savepoint, so one failing fire cannot undo the others.
type Sweeper struct {
	pool       *pgxpool.Pool
	interval   time.Duration
	authorizer FireAuthorizer
}

// FireIntent is the non-secret record view an authorization implementation
// must reauthorize just before a scheduler creates its owner effect. It is
// deliberately separate from the task template body.
type FireIntent struct {
	OrgID          string
	SpaceRef       string
	SubjectID      string
	ScheduleID     string
	FireKey        string
	TemplateDigest string
	IdempotencyKey string
}

// FireAuthorizer obtains and verifies fresh current authority for exactly one
// claimed cron slot. A long-lived schedule record is not authority by itself.
type FireAuthorizer interface {
	AuthorizeFire(context.Context, FireIntent) error
}

// NewSweeper constructs a sweeper that ticks every minute.
func NewSweeper(pool *pgxpool.Pool, authorizers ...FireAuthorizer) *Sweeper {
	var authorizer FireAuthorizer
	if len(authorizers) == 1 {
		authorizer = authorizers[0]
	}
	return &Sweeper{pool: pool, interval: time.Minute, authorizer: authorizer}
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
		// err names each failed schedule with its org id; fired still counts the
		// fires that committed in the same batch.
		slog.Warn("cron sweep reported errors", "fired", fired, "error", err)
	}
	if fired > 0 {
		slog.Info("cron sweep fired schedules", "count", fired)
	}
}

type dueSchedule struct {
	id             string
	orgID          string
	spaceRef       string
	subjectID      string
	templateDigest string
	expr           string
	tz             string
	template       json.RawMessage
	nextFire       *time.Time
}

// RunOnce scans due schedules and fires each exactly once. It returns the number
// of schedules whose task fired and committed. Schedules being initialized for the
// first time, or whose expression is invalid, do not count, and neither does a
// failed fire.
//
// A statement that fails is undone by its schedule's savepoint, so the other due
// schedules in the batch still fire and commit. A failed fire is recorded on
// cron_fires with status=failed and the schedule advances past that slot, which is
// the failed-fire path that fireOne and RunOnce have always taken for a failed task
// creation. Any other failure, including a failed fire whose record cannot be written,
// leaves the schedule due so the next tick retries it. Every failed schedule is named
// in the returned error together with its org id. A partial sweep therefore returns
// its committed count and a non-nil error, and callers must read both. A batch that
// cannot be scanned or committed returns 0 and an error.
func (s *Sweeper) RunOnce(ctx context.Context) (int, error) {
	if s.authorizer == nil {
		return 0, fmt.Errorf("cron schedule fire authorizer is not configured")
	}
	now := time.Now().UTC()
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return 0, err
	}
	defer func() { _ = tx.Rollback(ctx) }()

	rows, err := tx.Query(ctx, `
		SELECT id, org_id, space_ref, creator_subject_id, template_digest,
		       schedule_expr, timezone, task_template, next_fire_at
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
		if err := rows.Scan(&d.id, &d.orgID, &d.spaceRef, &d.subjectID, &d.templateDigest,
			&d.expr, &d.tz, &d.template, &d.nextFire); err != nil {
			rows.Close()
			return 0, err
		}
		dues = append(dues, d)
	}
	rows.Close()
	// rows.Next also returns false when the stream fails, so only Err tells a short
	// batch from a complete one.
	if err := rows.Err(); err != nil {
		return 0, err
	}

	fired := 0
	var failed []error
	for _, d := range dues {
		committed, err := s.sweepDue(ctx, tx, d, now)
		if err != nil {
			failed = append(failed, fmt.Errorf("cron schedule %s (org %s): %w", d.id, d.orgID, err))
			continue
		}
		if committed {
			fired++
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return 0, errors.Join(err, errors.Join(failed...))
	}
	return fired, errors.Join(failed...)
}

// sweepDue handles one due schedule inside its own savepoint and reports whether
// its task fired. An error means the schedule's writes were undone. A failed fire is
// then recorded as failed and the schedule advances past its slot; any other failure,
// or a failed fire whose record cannot be written either, leaves the schedule due for
// the next tick.
func (s *Sweeper) sweepDue(ctx context.Context, tx pgx.Tx, d dueSchedule, now time.Time) (bool, error) {
	next, nerr := NextFrom(d.expr, d.tz, now)
	if nerr != nil {
		// A malformed expression can never fire; disable it so the sweep does
		// not re-select it every tick.
		slog.Warn("disabling cron schedule with invalid expression",
			"id", d.id, "expr", d.expr, "error", nerr)
		return false, inSavepoint(ctx, tx, func(sp pgx.Tx) error {
			_, err := sp.Exec(ctx, `UPDATE cron_schedules SET enabled=false, updated_at=$1 WHERE id=$2`, now, d.id)
			return err
		})
	}
	if d.nextFire == nil {
		// First observation: initialize next_fire_at only; do not fire on the
		// tick that discovers the schedule.
		return false, inSavepoint(ctx, tx, func(sp pgx.Tx) error {
			_, err := sp.Exec(ctx, `UPDATE cron_schedules SET next_fire_at=$1, updated_at=$2 WHERE id=$3`, next, now, d.id)
			return err
		})
	}
	fireKey := d.nextFire.UTC().Format(time.RFC3339Nano)
	if err := s.authorizer.AuthorizeFire(ctx, FireIntent{
		OrgID: d.orgID, SpaceRef: d.spaceRef, SubjectID: d.subjectID,
		ScheduleID: d.id, FireKey: fireKey, TemplateDigest: d.templateDigest,
		IdempotencyKey: d.id + ":" + fireKey,
	}); err != nil {
		slog.Warn("disabling cron schedule after fresh authorization failed", "schedule", d.id, "error", err)
		return false, inSavepoint(ctx, tx, func(sp pgx.Tx) error {
			if _, err := sp.Exec(ctx, `UPDATE cron_schedules SET enabled=false, updated_at=$1 WHERE id=$2`, now, d.id); err != nil {
				return err
			}
			_, err := sp.Exec(ctx, `INSERT INTO cron_fires (id, schedule_id, fired_at, status, error) VALUES ($1,$2,$3,'failed',$4)`,
				"cronfire_"+uuid.New().String(), d.id, now, "fresh schedule authorization failed")
			return err
		})
	}
	ferr := inSavepoint(ctx, tx, func(sp pgx.Tx) error {
		if err := s.fireOne(ctx, sp, d, now); err != nil {
			return err
		}
		_, err := sp.Exec(ctx,
			`UPDATE cron_schedules SET last_fire_at=$1, next_fire_at=$2, updated_at=$1 WHERE id=$3`,
			now, next, d.id)
		return err
	})
	if ferr == nil {
		return true, nil
	}
	// The fire was undone. Record it as failed and advance past its slot, as RunOnce
	// and fireOne have always done for a failed task creation.
	rerr := inSavepoint(ctx, tx, func(sp pgx.Tx) error {
		if _, err := sp.Exec(ctx,
			`INSERT INTO cron_fires (id, schedule_id, fired_at, status, error, fire_key) VALUES ($1,$2,$3,'failed',$4,$5)`,
			"cronfire_"+uuid.New().String(), d.id, now, ferr.Error(), fireKey); err != nil {
			return err
		}
		_, err := sp.Exec(ctx,
			`UPDATE cron_schedules SET last_fire_at=$1, next_fire_at=$2, updated_at=$1 WHERE id=$3`,
			now, next, d.id)
		return err
	})
	return false, errors.Join(ferr, rerr)
}

// inSavepoint runs fn inside a savepoint of tx. When fn fails, the savepoint is
// rolled back, which undoes fn's statements and leaves tx usable for the rest of the
// batch. Without a savepoint, one failed statement aborts the whole batch
// transaction: every later statement fails with SQLSTATE 25P02, and the commit rolls
// everything back.
func inSavepoint(ctx context.Context, tx pgx.Tx, fn func(pgx.Tx) error) error {
	sp, err := tx.Begin(ctx)
	if err != nil {
		return err
	}
	if err := fn(sp); err != nil {
		if rerr := sp.Rollback(ctx); rerr != nil {
			return errors.Join(err, rerr)
		}
		return err
	}
	return sp.Commit(ctx)
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
func taskConfigJSON(template []byte, intent *FireIntent) []byte {
	trimmed := bytes.TrimSpace(template)
	if len(trimmed) == 0 {
		trimmed = []byte("{}")
	}
	// Only a JSON object is a usable template; an array or scalar would make
	// dispatchPlan's unmarshal fail and turn every fire of this schedule into a
	// failed task.
	var probe map[string]json.RawMessage
	if err := json.Unmarshal(trimmed, &probe); err != nil {
		probe = make(map[string]json.RawMessage)
	}
	if probe == nil {
		probe = make(map[string]json.RawMessage)
	}
	// The intent is non-secret and is deliberately persisted with the task so
	// the workflow dispatcher can ask Control again immediately before the
	// Temporal handoff. Never persist the short-lived signed decision itself:
	// Temporal histories and task config are not a bearer-token store.
	if intent != nil {
		probe["schedule_fire_intent"], _ = json.Marshal(intent)
	}
	encoded, err := json.Marshal(probe)
	if err != nil {
		return []byte("{}")
	}
	return encoded
}

// fireOne creates a task from the schedule's template and records a pending
// cron_fires row. It stops at the first failed statement and returns that error,
// because Postgres rejects every later statement on the aborted transaction. The
// caller rolls the fire's savepoint back, so a failed fire leaves no task or pending
// row behind.
func (s *Sweeper) fireOne(ctx context.Context, tx pgx.Tx, d dueSchedule, now time.Time) error {
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
	// The fire key is the schedule's slot. Session Core matches a scheduled run
	// back to this fire through it (build plan WS20 S1).
	fireKey := d.nextFire.UTC().Format(time.RFC3339Nano)
	_, terr := tx.Exec(ctx, `
		INSERT INTO tasks (id, org_id, kind, title, description, assignee, status,
		    priority, config_json, scheduled_at, created_at, updated_at)
		VALUES ($1,$2,$3,$4,$5,$6,'created',$7,$8,$9,$9,$9)
	`, taskID, d.orgID, tpl.Kind, tpl.Title, tpl.Description, tpl.Assignee, tpl.Priority,
		taskConfigJSON(d.template, &FireIntent{
			OrgID: d.orgID, SpaceRef: d.spaceRef, SubjectID: d.subjectID,
			ScheduleID: d.id, FireKey: d.nextFire.UTC().Format(time.RFC3339Nano),
			TemplateDigest: d.templateDigest, IdempotencyKey: d.id + ":" + d.nextFire.UTC().Format(time.RFC3339Nano),
		}), now)
	if terr != nil {
		return terr
	}
	_, err := tx.Exec(ctx,
		`INSERT INTO cron_fires (id, schedule_id, task_id, fired_at, status, fire_key) VALUES ($1,$2,$3,$4,'pending',$5)`,
		fireID, d.id, taskID, now, fireKey)
	return err
}

// Package taskexec runs durable tasks: it claims `created` tasks, advances them
// through the lifecycle (created -> running -> failed on dispatch error), and
// hands each to a Dispatcher that performs the actual work. Claiming uses
// FOR UPDATE SKIP LOCKED so multiple replicas never grab the same task.
//
// Scope boundary: this layer owns the CLAIM + LIFECYCLE + hand-off. The
// Dispatcher owns execution.
//
// Two dispatchers exist and the difference matters:
//
//   - WorkflowDispatcher (production) starts a durable Temporal run through
//     orchestrator-core's StartWorkflow RPC and returns synchronously, so a
//     failure is a failure the executor can record. This is what makes the
//     executor safe to enable.
//   - NatsDispatcher only publishes mp.v1.capability.task.dispatched and calls
//     that success. If nothing consumes the subject the task stays in `running`
//     forever, which is precisely why the executor shipped disabled. It remains
//     for deployments that run their own consumer of that subject.
//
// Consequently the executor's default is gated on having a dispatcher that can
// actually complete the hand-off — see cmd/main.go.
package taskexec

import (
	"context"
	"encoding/json"
	"log/slog"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/pkg/publisher"
	"github.com/triodelab/model-plane/services/capability-core/internal/reconcile"
)

// TaskRef identifies a claimed task handed to a Dispatcher.
type TaskRef struct {
	ID    string
	OrgID string
	Kind  string
}

// Dispatcher performs (or hands off) the actual work of a claimed task. The
// executor owns the lifecycle; the Dispatcher owns execution. A nil return means
// "accepted — a runner will complete it"; an error marks the task failed.
type Dispatcher interface {
	Dispatch(ctx context.Context, task TaskRef) error
}

// NatsDispatcher publishes a task-dispatch event (mp.v1.capability.task.dispatched)
// for a downstream Model-Plane runner to execute and complete.
//
// WARNING: a successful publish is NOT a successful dispatch. Nothing in the
// Model Plane consumes this subject as a work request, so on its own this
// dispatcher leaves every claimed task in `running` indefinitely. Use
// WorkflowDispatcher unless the deployment supplies its own consumer.
type NatsDispatcher struct{ pub publisher.EventPublisher }

// NewNatsDispatcher wraps a publisher.
func NewNatsDispatcher(pub publisher.EventPublisher) *NatsDispatcher {
	return &NatsDispatcher{pub: pub}
}

// Dispatch publishes the task-dispatch event.
func (d *NatsDispatcher) Dispatch(ctx context.Context, task TaskRef) error {
	return reconcile.Emit(ctx, d.pub, reconcile.KindTask, reconcile.ActionDispatched, task.ID, task.OrgID)
}

// Executor is the claim + lifecycle worker.
type Executor struct {
	pool       *pgxpool.Pool
	dispatcher Dispatcher
	interval   time.Duration
	batch      int
}

// NewExecutor constructs an executor that polls every 15s and claims up to 20
// tasks per sweep.
func NewExecutor(pool *pgxpool.Pool, dispatcher Dispatcher) *Executor {
	return &Executor{pool: pool, dispatcher: dispatcher, interval: 15 * time.Second, batch: 20}
}

// isAutoExecutable reports whether a task of this kind should be picked up by
// the executor. `manual` tasks are user-facing tracking items and are left
// alone; everything else (agent/cron/workflow/shell) is auto-run.
func isAutoExecutable(kind string) bool {
	return kind != "" && kind != "manual"
}

// Start runs the claim loop until ctx is cancelled. Errors are logged, not fatal.
func (e *Executor) Start(ctx context.Context) {
	ticker := time.NewTicker(e.interval)
	defer ticker.Stop()
	e.sweepAndLog(ctx)
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			e.sweepAndLog(ctx)
		}
	}
}

func (e *Executor) sweepAndLog(ctx context.Context) {
	claimed, err := e.RunOnce(ctx)
	if err != nil {
		slog.Warn("task executor sweep failed", "error", err)
		return
	}
	if claimed > 0 {
		slog.Info("task executor claimed tasks", "count", claimed)
	}
}

// RunOnce claims a batch of created tasks, marks them running, and dispatches
// each. Returns the number of tasks claimed. Claiming is single-flight across
// replicas (FOR UPDATE SKIP LOCKED); dispatch happens after the claim commits so
// a slow runner does not hold the row lock.
func (e *Executor) RunOnce(ctx context.Context) (int, error) {
	now := time.Now().UTC()
	tx, err := e.pool.Begin(ctx)
	if err != nil {
		return 0, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	rows, err := tx.Query(ctx, `
		SELECT id, org_id, kind
		FROM tasks
		WHERE status = 'created' AND deleted_at IS NULL AND kind <> 'manual'
		ORDER BY priority DESC, created_at ASC
		FOR UPDATE SKIP LOCKED
		LIMIT $1
	`, e.batch)
	if err != nil {
		return 0, err
	}
	var claimed []TaskRef
	for rows.Next() {
		var t TaskRef
		if err := rows.Scan(&t.ID, &t.OrgID, &t.Kind); err != nil {
			rows.Close()
			return 0, err
		}
		// Belt-and-suspenders: the SQL already excludes `manual`, but the policy
		// authority is isAutoExecutable — never run a non-auto-executable task.
		if !isAutoExecutable(t.Kind) {
			continue
		}
		claimed = append(claimed, t)
	}
	rows.Close()

	for _, t := range claimed {
		if _, err := tx.Exec(ctx,
			`UPDATE tasks SET status='running', started_at=$1, updated_at=$1 WHERE id=$2`,
			now, t.ID); err != nil {
			return 0, err
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return 0, err
	}
	committed = true

	// Dispatch outside the claim tx. A dispatch failure flips the task to failed
	// so it is not stranded in running; success leaves it running for the started
	// run to complete.
	for _, t := range claimed {
		if err := e.dispatcher.Dispatch(ctx, t); err != nil {
			slog.Warn("task dispatch failed", "task", t.ID, "error", err)
			e.failTask(ctx, t, err)
		}
	}
	return len(claimed), nil
}

// failTask moves an undispatchable task to `failed` WITH a recorded reason.
//
// Both writes go in one transaction: a task must never end up marked failed with
// no explanation of why, which is all an operator has to work from once the
// process log has rotated away.
func (e *Executor) failTask(ctx context.Context, task TaskRef, cause error) {
	now := time.Now().UTC()
	tx, err := e.pool.Begin(ctx)
	if err != nil {
		slog.Error("task failure not recorded", "task", task.ID, "error", err)
		return
	}
	defer func() { _ = tx.Rollback(ctx) }()

	tag, err := tx.Exec(ctx,
		`UPDATE tasks SET status='failed', completed_at=$1, updated_at=$1 WHERE id=$2 AND status='running'`,
		now, task.ID)
	if err != nil {
		slog.Error("task failure not recorded", "task", task.ID, "error", err)
		return
	}
	if tag.RowsAffected() == 0 {
		// Another actor already moved it out of `running`; leave their state
		// alone rather than overwriting it with a failure.
		return
	}

	payload, merr := json.Marshal(map[string]string{
		"reason": cause.Error(),
		"stage":  "dispatch",
	})
	if merr != nil {
		payload = []byte(`{"reason":"dispatch failed","stage":"dispatch"}`)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO task_events (id, task_id, event_type, actor, payload, ts)
		VALUES ($1, $2, 'failed', $3, $4, $5)
	`, "taskevt_"+uuid.NewString(), task.ID, dispatchActor, payload, now); err != nil {
		slog.Error("task failure reason not recorded", "task", task.ID, "error", err)
		return
	}
	if err := tx.Commit(ctx); err != nil {
		slog.Error("task failure not committed", "task", task.ID, "error", err)
	}
}

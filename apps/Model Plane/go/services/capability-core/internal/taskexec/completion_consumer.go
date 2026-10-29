package taskexec

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/nats-io/nats.go"

	"github.com/triodelab/model-plane/pkg/envelope"
)

// RunEventSubject is the run-lifecycle subject filter. RUN_COMPLETED and
// RUN_FAILED envelopes for every run arrive here; only the ones whose run id is
// a task id belong to us.
const RunEventSubject = "mp.v1.run.*.event"

// runCompletionQueue is the NATS queue group so N capability-core replicas share
// the stream instead of each closing the same task.
const runCompletionQueue = "capability-core-task-completion"

// completionActor identifies this component in task_events.actor.
const completionActor = "capability-core/task-completion"

// terminalStatus maps a run lifecycle event to the task status it implies.
// Anything else (RUN_STARTED, step events) is not terminal and is ignored.
func terminalStatus(eventType string) (status, taskEvent string, ok bool) {
	switch eventType {
	case "RUN_COMPLETED":
		return "completed", "completed", true
	case "RUN_FAILED":
		return "failed", "failed", true
	default:
		return "", "", false
	}
}

// RunCompletionConsumer closes the task lifecycle when the run a task started
// reaches a terminal state.
//
// Without it the executor would be exactly as unsafe as before: the workflow
// dispatcher starts a real run, but nothing would ever move the task out of
// `running`, so every cron-fired task would strand there forever — the failure
// mode the executor originally shipped disabled to avoid. Because the dispatcher
// uses the task id AS the run id, the correlation needs no mapping table: a
// terminal event whose run id matches a `running` task in the same org closes
// that task.
type RunCompletionConsumer struct {
	pool *pgxpool.Pool
}

// NewRunCompletionConsumer constructs the consumer.
func NewRunCompletionConsumer(pool *pgxpool.Pool) (*RunCompletionConsumer, error) {
	if pool == nil {
		return nil, errors.New("taskexec: run completion consumer requires a database pool")
	}
	return &RunCompletionConsumer{pool: pool}, nil
}

// Run subscribes until ctx is cancelled. Per-message errors are logged, never
// fatal.
func (c *RunCompletionConsumer) Run(ctx context.Context, nc *nats.Conn) error {
	sub, err := nc.QueueSubscribe(RunEventSubject, runCompletionQueue, func(msg *nats.Msg) {
		closed, herr := c.Handle(ctx, msg.Data)
		switch {
		case herr != nil:
			slog.Warn("task completion from run event failed", "subject", msg.Subject, "error", herr)
		case closed != "":
			slog.Info("task closed by run event", "task", closed, "subject", msg.Subject)
		}
	})
	if err != nil {
		return fmt.Errorf("taskexec: subscribe %s: %w", RunEventSubject, err)
	}
	slog.Info("task completion consumer started", "subject", RunEventSubject, "queue", runCompletionQueue)
	<-ctx.Done()
	_ = sub.Unsubscribe()
	return nil
}

// Handle applies one run-lifecycle envelope. It returns the task id it closed,
// or "" when the event was not a terminal event for a task-backed run — which is
// the common case, since this subject carries every run in the plane.
func (c *RunCompletionConsumer) Handle(ctx context.Context, data []byte) (string, error) {
	var env envelope.Envelope
	if err := json.Unmarshal(data, &env); err != nil {
		return "", fmt.Errorf("taskexec: decode run envelope: %w", err)
	}
	status, taskEvent, ok := terminalStatus(env.EventType)
	if !ok {
		return "", nil
	}
	runID := runIDFrom(&env)
	// Only the dispatcher's own run ids can be task ids, and those are always
	// task-prefixed. Skipping everything else keeps normal chat runs from
	// touching the tasks table at all.
	if runID == "" || !strings.HasPrefix(runID, "task_") {
		return "", nil
	}
	orgID := strings.TrimSpace(env.OrgID)
	if orgID == "" {
		return "", fmt.Errorf("taskexec: run %s event has no org_id", runID)
	}

	reason := ""
	if status == "failed" {
		reason = failureReason(env.Payload)
	}
	return c.closeTask(ctx, runID, orgID, status, taskEvent, reason)
}

// closeTask advances the task and records why, in one transaction.
//
// The org_id predicate is the tenant guard: a run event can only close a task
// belonging to the same organization, so a crafted envelope naming another
// tenant's task id changes nothing. The status='running' predicate makes
// redelivery a no-op.
func (c *RunCompletionConsumer) closeTask(
	ctx context.Context,
	taskID, orgID, status, taskEvent, reason string,
) (string, error) {
	now := time.Now().UTC()
	tx, err := c.pool.Begin(ctx)
	if err != nil {
		return "", err
	}
	defer func() { _ = tx.Rollback(ctx) }()

	tag, err := tx.Exec(ctx, `
		UPDATE tasks SET status = $1, completed_at = $2, updated_at = $2
		WHERE id = $3 AND org_id = $4 AND status = 'running' AND deleted_at IS NULL
	`, status, now, taskID, orgID)
	if err != nil {
		return "", err
	}
	if tag.RowsAffected() == 0 {
		// Not a task-backed run, another org's id, or already closed.
		return "", nil
	}

	fields := map[string]string{"run_id": taskID}
	if reason != "" {
		fields["reason"] = reason
	}
	payload, merr := json.Marshal(fields)
	if merr != nil {
		payload = []byte(`{}`)
	}
	if _, err := tx.Exec(ctx, `
		INSERT INTO task_events (id, task_id, event_type, actor, payload, ts)
		VALUES ($1, $2, $3, $4, $5, $6)
	`, "taskevt_"+uuid.NewString(), taskID, taskEvent, completionActor, payload, now); err != nil {
		return "", err
	}
	if err := tx.Commit(ctx); err != nil {
		return "", err
	}
	return taskID, nil
}

// runIDFrom reads the run id from the payload, falling back to correlation_id.
// orchestrator-core sets both; the fallback keeps this working for any producer
// that only populates the envelope header.
func runIDFrom(env *envelope.Envelope) string {
	var payload struct {
		RunID string `json:"run_id"`
	}
	if len(env.Payload) > 0 {
		if err := json.Unmarshal(env.Payload, &payload); err == nil {
			if id := strings.TrimSpace(payload.RunID); id != "" {
				return id
			}
		}
	}
	return strings.TrimSpace(env.CorrelationID)
}

// failureReason extracts the run's failure reason for the audit row.
func failureReason(raw json.RawMessage) string {
	var payload struct {
		Reason string `json:"reason"`
	}
	if len(raw) == 0 {
		return ""
	}
	if err := json.Unmarshal(raw, &payload); err != nil {
		return ""
	}
	return strings.TrimSpace(payload.Reason)
}

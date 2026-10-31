package jobs

// The dispatcher's side of job control (WS15.L2): the execution timeout it gives
// a workflow, and the compare-and-set that marks a started job running. A cancel
// that reached control before the dispatcher marks the job running makes the
// write answer 409; the workflow the dispatcher just started is then terminated,
// so it does not run.

import (
	"context"
	"errors"
	"time"

	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/timeouts"
)

// errJobChanged is returned when control no longer has the job as accepted: a
// cancel (or a terminal event) got there first.
var errJobChanged = errors.New("job is no longer accepted")

// legacyExecutionTimeout is the dispatcher's execution timeout while the control
// loop is off, unchanged from before WS15.L2.
const legacyExecutionTimeout = 30 * time.Minute

// executionTimeout is the workflow execution timeout of a job: the per-kind value
// when the control loop is on, the legacy constant when it is off.
func (m *Manager) executionTimeout(j job) time.Duration {
	if !m.cfg.ControlLoop {
		return legacyExecutionTimeout
	}
	return timeouts.ExecutionTimeoutFor(j.Kind, j.Params)
}

// stopStarted terminates a workflow the dispatcher started for a job that control
// no longer has as accepted. Best effort: the workflow would otherwise run for a
// job that is already ended.
func (m *Manager) stopStarted(ctx context.Context, workflowID string) {
	if err := m.temporal.TerminateWorkflow(ctx, workflowID, "", "job cancelled before it started"); err != nil {
		log.Warn().Err(err).Str("workflow_id", workflowID).Msg("jobs dispatcher: terminate of a cancelled start failed")
	}
}

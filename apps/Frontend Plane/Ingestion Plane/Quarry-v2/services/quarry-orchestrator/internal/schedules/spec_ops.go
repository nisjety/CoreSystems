package schedules

import (
	"context"
	"time"

	"go.temporal.io/sdk/client"
)

// scheduleSpecFor is the Temporal calendar of a desired schedule: its cron, its
// zone and its jitter. A UTC or empty zone and zero jitter are left unset, so an
// existing schedule created before these fields existed is not changed.
func (m *Manager) scheduleSpecFor(spec ScheduleSpec) client.ScheduleSpec {
	s := client.ScheduleSpec{
		CronExpressions: []string{spec.Cron},
		TimeZoneName:    normalizeTimeZone(spec.TimeZone),
	}
	if spec.JitterS > 0 {
		s.Jitter = time.Duration(spec.JitterS) * time.Second
	}
	return s
}

// scheduleActionFor is the workflow a fire starts: the wrapper with its args, on
// the orchestrator task queue, with the action timeout derived from the job timeout.
func (m *Manager) scheduleActionFor(spec ScheduleSpec) *client.ScheduleWorkflowAction {
	return &client.ScheduleWorkflowAction{
		ID:                       spec.ID + "-wf",
		Workflow:                 spec.Workflow,
		Args:                     spec.Args,
		TaskQueue:                m.cfg.TaskQueue,
		WorkflowExecutionTimeout: actionTimeout(spec.ExecutionTimeoutS),
	}
}

// policyFor is the schedule policy on update. The overlap policy and
// pause-on-failure come from the spec. The catch-up window comes from the spec
// when set, and otherwise keeps what Temporal already holds.
func (m *Manager) policyFor(spec ScheduleSpec, current *client.SchedulePolicies) *client.SchedulePolicies {
	p := client.SchedulePolicies{
		Overlap:        overlapPolicy(spec.Overlap),
		PauseOnFailure: spec.PauseOnFailure,
	}
	if spec.CatchupWindowS > 0 {
		p.CatchupWindow = time.Duration(spec.CatchupWindowS) * time.Second
	} else if current != nil {
		p.CatchupWindow = current.CatchupWindow
	}
	return &p
}

// pauseIfRunning pauses an existing Temporal schedule whose desired workflow is
// now empty (its control row no longer maps to a workflow). It never deletes the
// schedule. Failures are logged and retried on the next pass.
func (m *Manager) pauseIfRunning(ctx context.Context, sc client.ScheduleClient, id string) {
	h := sc.GetHandle(ctx, id)
	if desc, err := h.Describe(ctx); err == nil && desc != nil &&
		desc.Schedule.State != nil && desc.Schedule.State.Paused {
		return
	}
	if err := h.Pause(ctx, client.SchedulePauseOptions{Note: "reconciled: no workflow mapped"}); err != nil {
		m.log.Error().Err(err).Str("id", id).Msg("pause schedule without a workflow failed")
	}
}

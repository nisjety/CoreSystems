// Package workflows: ScheduledJobWF, the wrapper a scrape, crawl or batch
// schedule fires (WS15.S2, decision D12: a wrapper workflow as the schedule
// action, not the job workflows directly and not a tick row).
//
// Per fire the wrapper:
//  1. asks the edge for a fire-time preflight (S2 always allows; S4 adds ZDR
//     posture and entitlement), and registers a terminal skipped job when the
//     edge denies;
//  2. registers a running control job for the fire (origin schedule), so the
//     fire is a tracked job the way a user-started job is;
//  3. runs the matching job workflow (ScrapeJobWF, CrawlJobWF or BatchJobWF) as
//     a child whose workflow id is quarry-job-<jobID>, and turns a child error
//     into run_failed or run_timed_out with schedule fields and no URL.
//
// Determinism: the wrapper reads no clock, no environment and no randomness.
// Its run id derives from its own execution (as ChangeMonitorWF does). Crawl
// bounds, the execution timeout and the overlap policy arrive in the input,
// stamped by the schedule reconciler when it materializes the schedule.
package workflows

import (
	"errors"
	"fmt"
	"strings"
	"time"

	enumspb "go.temporal.io/api/enums/v1"
	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

const (
	// ScheduledOverlapCancel is the schedule overlap policy that cancels the
	// previous fire. The wrapper then signals its child to cancel, through a
	// disconnected context, and waits up to scheduledCancelWait for it to stop.
	ScheduledOverlapCancel = "cancel"
	scheduledCancelWait    = 2 * time.Minute

	// Reason classes written on skipped and failed fires. Closed vocabulary;
	// never a raw error string or a URL.
	ScheduledReasonPreflightDenied = "preflight_denied"
	ScheduledReasonInvalidParams   = "invalid_params"
	ScheduledReasonChildFailed     = "child_failed"
	ScheduledReasonChildTimedOut   = "child_timed_out"
)

// ScheduledJobInput is the argument of ScheduledJobWF. Control writes the
// schedule identity, kind and params (args of the Temporal schedule action);
// the reconciler adds the resolved crawl bounds into params, the overlap policy
// and the execution timeout.
type ScheduledJobInput struct {
	ScheduleID        string         `json:"schedule_id"`
	OrgID             string         `json:"org_id"`
	CreatedBy         string         `json:"created_by,omitempty"`
	Kind              string         `json:"kind"`
	Params            map[string]any `json:"params,omitempty"`
	OverlapPolicy     string         `json:"overlap_policy,omitempty"`
	ExecutionTimeoutS int64          `json:"execution_timeout_s,omitempty"`
}

// ScheduledJobWF runs one fire of a scrape, crawl or batch schedule. It is
// registered by name in the orchestrator's main.go with the activities injected.
func ScheduledJobWF(ctx workflow.Context, in ScheduledJobInput, a *activities.Activities) error {
	ctx = workflow.WithActivityOptions(ctx, defaultActivityOpts())
	info := workflow.GetInfo(ctx)
	runID := "run_" + strings.ReplaceAll(info.WorkflowExecution.RunID, "-", "")

	var pre activities.PreflightResult
	if err := workflow.ExecuteActivity(ctx, a.PreflightFire, activities.PreflightInput{
		ScheduleID: in.ScheduleID,
		OrgID:      in.OrgID,
	}).Get(ctx, &pre); err != nil {
		return err
	}
	if !pre.Allow {
		reason := pre.Reason
		if reason == "" {
			reason = ScheduledReasonPreflightDenied
		}
		_, err := registerScheduledRun(ctx, a, in, runID, "skipped", reason, info.WorkflowExecution.ID)
		return err
	}

	reg, err := registerScheduledRun(ctx, a, in, runID, "running", "", info.WorkflowExecution.ID)
	if err != nil {
		return err
	}
	ids := runIDs{RunID: runID, JobID: reg.JobID}

	child, err := startScheduledChild(ctx, in, runID, reg.JobID)
	if err != nil {
		_ = emitScheduleOutcome(ctx, a, ids, quarrycontracts.EvtRunFailed, in, ScheduledReasonInvalidParams)
		return err
	}

	// Wait for the child, or for this fire to be cancelled. A plain child.Get
	// would stay blocked until the child closed, so the cancel signal below
	// could only ever reach a child that had already stopped.
	var childErr error
	childDone := false
	await := workflow.NewSelector(ctx)
	await.AddFuture(child, func(f workflow.Future) {
		childErr = f.Get(ctx, nil)
		childDone = true
	})
	await.AddReceive(ctx.Done(), func(workflow.ReceiveChannel, bool) {})
	await.Select(ctx)
	if childDone && childErr == nil {
		return nil
	}

	// The schedule's overlap policy cancelled this fire. Ask the child to stop
	// on a context that survives the cancellation, then wait for it.
	if ctx.Err() != nil {
		if in.OverlapPolicy == ScheduledOverlapCancel {
			disc, cancel := workflow.NewDisconnectedContext(ctx)
			defer cancel()
			_ = child.SignalChildWorkflow(disc, SignalCancel, nil).Get(disc, nil)
			// Wait for the child to stop, but no longer than scheduledCancelWait.
			wait := workflow.NewSelector(disc)
			wait.AddFuture(child, func(f workflow.Future) { _ = f.Get(disc, nil) })
			wait.AddFuture(workflow.NewTimer(disc, scheduledCancelWait), func(workflow.Future) {})
			wait.Select(disc)
		}
		return nil
	}

	evt := quarrycontracts.EvtRunFailed
	reason := ScheduledReasonChildFailed
	var timeoutErr *temporal.TimeoutError
	if errors.As(childErr, &timeoutErr) {
		evt = quarrycontracts.EvtRunTimedOut
		reason = ScheduledReasonChildTimedOut
	}
	_ = emitScheduleOutcome(ctx, a, ids, evt, in, reason)
	// Returning the error lets the schedule's pause-on-failure option see it.
	return childErr
}

// scheduledChildOptions are the child options of one fire: the child keeps its
// own run after the wrapper closes only when cancelled, and it runs on the
// wrapper's task queue.
func scheduledChildOptions(ctx workflow.Context, in ScheduledJobInput, jobID string) workflow.ChildWorkflowOptions {
	opts := workflow.ChildWorkflowOptions{
		WorkflowID:        "quarry-job-" + jobID,
		TaskQueue:         workflow.GetInfo(ctx).TaskQueueName,
		ParentClosePolicy: enumspb.PARENT_CLOSE_POLICY_REQUEST_CANCEL,
	}
	if in.ExecutionTimeoutS > 0 {
		opts.WorkflowExecutionTimeout = time.Duration(in.ExecutionTimeoutS) * time.Second
	}
	return opts
}

// startScheduledChild builds the child input with the shared builders and
// starts the job workflow for the fire's kind.
func startScheduledChild(ctx workflow.Context, in ScheduledJobInput, runID, jobID string) (workflow.ChildWorkflowFuture, error) {
	childCtx := workflow.WithChildOptions(ctx, scheduledChildOptions(ctx, in, jobID))
	switch in.Kind {
	case "scrape":
		child, err := ScrapeInputFromParams(in.Params, runID, jobID)
		if err != nil {
			return nil, err
		}
		return workflow.ExecuteChildWorkflow(childCtx, "ScrapeJobWF", child), nil
	case "crawl":
		child, err := CrawlInputFromParams(in.Params, runID, jobID,
			uintParam(in.Params, "max_depth"), uintParam(in.Params, "max_pages"))
		if err != nil {
			return nil, err
		}
		return workflow.ExecuteChildWorkflow(childCtx, "CrawlJobWF", child), nil
	case "batch":
		child, err := BatchInputFromParams(in.Params, runID, jobID)
		if err != nil {
			return nil, err
		}
		return workflow.ExecuteChildWorkflow(childCtx, "BatchJobWF", child), nil
	}
	return nil, fmt.Errorf("scheduled job kind %q has no workflow", in.Kind)
}

// registerScheduledRun asks control to record the fire as a job (running) or as
// a terminal skipped job. Control takes org, kind and params from the schedule
// row, never from this call.
func registerScheduledRun(ctx workflow.Context, a *activities.Activities, in ScheduledJobInput, runID, status, reason, workflowID string) (activities.RegisterRunResult, error) {
	var out activities.RegisterRunResult
	err := workflow.ExecuteActivity(ctx, a.RegisterScheduledRun, activities.RegisterRunInput{
		ScheduleID: in.ScheduleID,
		RunKey:     runID,
		RunID:      runID,
		Status:     status,
		Reason:     reason,
		WorkflowID: workflowID,
	}).Get(ctx, &out)
	return out, err
}

// emitScheduleOutcome writes run_failed or run_timed_out for a fire. The payload
// carries the schedule fields and a reason class, and no URL.
func emitScheduleOutcome(ctx workflow.Context, a *activities.Activities, ids runIDs, evt quarrycontracts.EventType, in ScheduledJobInput, reason string) error {
	return emitEvent(ctx, a, ids, evt, map[string]any{
		"schedule_id": in.ScheduleID,
		"created_by":  in.CreatedBy,
		"org_id":      in.OrgID,
		"reason":      reason,
	}, ids.RunID, string(evt))
}

// uintParam reads a bound stored in the params map as a number.
func uintParam(params map[string]any, key string) uint32 {
	return UintFromParam(params[key])
}

package workflows

import (
	"fmt"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// The step loop drives an agent run turn-by-turn against execution-core.
//
// Historically the ENTIRE MaxTurns loop ran inside one Temporal activity
// (ExecuteStepLoopActivity): a worker crash mid-loop restarted the run from
// turn 0, losing every completed turn and re-spending its work. The durable
// design here moves loop control into workflow code and makes every turn its
// own ExecuteStepActivity, so each completed turn is a checkpoint in event
// history — a crash replays completed turns and resumes at the interrupted one.
//
// This mirrors the resume-safe evaluator-optimizer driver, where each
// generator/judge leg is its own activity (see evaluator_optimizer.go).
const (
	// stepLoopPerTurnChange gates the migration from the monolithic
	// ExecuteStepLoopActivity to the durable per-turn executeStepLoop. In-flight
	// runs whose history recorded workflow.DefaultVersion keep replaying the
	// legacy single-activity path; new runs take the per-turn path. The activity
	// stays registered so those old histories remain replay-compatible.
	stepLoopPerTurnChange  = "step-loop-per-turn"
	stepLoopPerTurnVersion = 1
)

// runStepLoop dispatches the step loop under the GetVersion gate so both
// in-flight and new runs stay replay-safe. ctx MUST already carry
// ActivityOptions (workflow.WithActivityOptions). Its StepLoopOutput/error are
// shaped identically to the legacy activity, so a caller migrates by swapping
// its ExecuteActivity("ExecuteStepLoopActivity", ...) call for this helper.
func runStepLoop(ctx workflow.Context, in activities.StepLoopInput) (activities.StepLoopOutput, error) {
	if workflow.GetVersion(ctx, stepLoopPerTurnChange, workflow.DefaultVersion, stepLoopPerTurnVersion) == workflow.DefaultVersion {
		var out activities.StepLoopOutput
		err := workflow.ExecuteActivity(ctx, "ExecuteStepLoopActivity", in).Get(ctx, &out)
		return out, err
	}
	return executeStepLoop(ctx, in)
}

// executeStepLoop is the durable, resume-safe step-loop driver: the workflow —
// not one activity — owns the turn loop, dispatching each turn as its own
// ExecuteStepActivity (a durable checkpoint in event history). It preserves the
// legacy loop semantics exactly: the same default turn cap, a non-fatal
// "pending" turn when execution-core is unavailable, a break on the first
// Completed or NeedsApproval turn, and the same "executed N steps for run X"
// summary.
//
// ctx MUST already carry ActivityOptions (workflow.WithActivityOptions); the
// per-turn StartToCloseTimeout now bounds ONE turn rather than the whole loop.
// On a turn error the partial steps gathered so far are returned alongside the
// error, matching the legacy activity's early-return contract.
//
// Callers that need per-turn durability WITHOUT the version gate (e.g. a branch
// already run under a workflow-level GetVersion, such as WideResearchWorkflow's
// per-branch coroutines) call this directly; everyone else goes through
// runStepLoop.
func executeStepLoop(ctx workflow.Context, in activities.StepLoopInput) (activities.StepLoopOutput, error) {
	maxTurns := in.MaxTurns
	if maxTurns <= 0 {
		maxTurns = defaultMaxTurns
	}
	var steps []activities.StepResult
	for i := range maxTurns {
		var step activities.StepResult
		err := workflow.ExecuteActivity(ctx, "ExecuteStepActivity", activities.StepInput{
			RunID:     in.RunID,
			OrgID:     in.OrgID,
			UserID:    in.UserID,
			StepIndex: i,
		}).Get(ctx, &step)
		if err != nil {
			return activities.StepLoopOutput{Steps: steps, Completed: false}, err
		}
		steps = append(steps, step)
		if step.Completed || step.NeedsApproval {
			break
		}
	}
	return activities.StepLoopOutput{
		Steps:     steps,
		Completed: len(steps) > 0 && steps[len(steps)-1].Completed,
		Summary:   fmt.Sprintf("executed %d steps for run %s", len(steps), in.RunID),
	}, nil
}

// scheduledStepIntent is the Control-bound intent of one scheduled turn: only the immutable,
// template-bound identifiers and the contract digest, never a user-bound request.
func scheduledStepIntent(in ScheduledRunInput, i int) activities.ScheduledStepExecutionIntent {
	return activities.ScheduledStepExecutionIntent{
		OrgID:             in.OrgID,
		SpaceRef:          in.SpaceRef,
		SubjectID:         in.SubjectID,
		RunID:             in.RunID,
		ThreadID:          in.ThreadID,
		ScheduleID:        in.ScheduleID,
		FireKey:           in.FireKey,
		TemplateDigest:    in.TemplateDigest,
		StepID:            fmt.Sprintf("%s:step:%d", in.RunID, i),
		StepIndex:         uint32(i),
		PolicyDigest:      in.PolicyDigest,
		IdempotencyKey:    fmt.Sprintf("%s:step:%d", in.IdempotencyKey, i),
		ContractDigest:    in.ContractDigest,
		ContractExpiresAt: in.ContractExpiresAt,
	}
}

// scheduledAgentStepTimeout bounds one agent-lane step: execution-core's 240 s wall clock plus the
// time to record the receipt (WS20 S6).
const scheduledAgentStepTimeout = 6 * time.Minute

// executeScheduledAgentStep runs a fire under a stored-consent contract as one bounded agent-loop
// step, not the single-inference loop (WS20 S6). The step is never retried: an unknown outcome stops
// the run for reconciliation, as the single-inference loop does.
func executeScheduledAgentStep(ctx workflow.Context, in ScheduledRunInput) (activities.StepLoopOutput, error) {
	agentCtx := workflow.WithActivityOptions(ctx, workflow.ActivityOptions{
		StartToCloseTimeout: scheduledAgentStepTimeout,
		HeartbeatTimeout:    time.Minute,
		RetryPolicy:         &temporal.RetryPolicy{MaximumAttempts: 1},
	})
	var step activities.StepResult
	if err := workflow.ExecuteActivity(agentCtx, "ExecuteScheduledStepActivity", scheduledStepIntent(in, 0)).Get(ctx, &step); err != nil {
		return activities.StepLoopOutput{Completed: false}, err
	}
	if step.UnknownOutcome {
		return activities.StepLoopOutput{Steps: []activities.StepResult{step}, Completed: false}, temporal.NewNonRetryableApplicationError(
			"scheduled-step provider outcome is unknown; reconcile its durable receipt before retrying",
			"ScheduledStepUnknownOutcome",
			nil,
		)
	}
	return activities.StepLoopOutput{
		Steps:     []activities.StepResult{step},
		Completed: step.Completed,
		Summary:   fmt.Sprintf("executed one scheduled agent step for run %s", in.RunID),
	}, nil
}

// scheduledAgentOutcomeSummary names how an agent-lane run ended: failed when the step did not
// complete (a refused contract, an incomplete run), needs_review when an effect was withheld for
// the owner, completed otherwise. It carries counts only, never content (WS20 S6).
func scheduledAgentOutcomeSummary(runID string, out activities.StepLoopOutput) string {
	outcome, rounds, withheld := scheduledAgentOutcome(out)
	return fmt.Sprintf("scheduled run %s outcome %s: %d rounds, %d withheld effects", runID, outcome, rounds, withheld)
}

// scheduledAgentOutcome classifies how an agent-lane run ended and totals its counts: "failed"
// when the last step did not complete, "needs_review" when it completed with withheld effects,
// "completed" otherwise.
func scheduledAgentOutcome(out activities.StepLoopOutput) (outcome string, rounds, withheld int) {
	for _, step := range out.Steps {
		rounds += step.RoundsExecuted
		withheld += step.WithheldEffects
	}
	completed := len(out.Steps) > 0 && out.Steps[len(out.Steps)-1].Completed
	outcome = "completed"
	switch {
	case !completed:
		outcome = "failed"
	case withheld > 0:
		outcome = "needs_review"
	}
	return outcome, rounds, withheld
}

// scheduledAgentFailureReason is the closed failure code an agent-lane run that did not complete
// its work is recorded under (FailRunActivity, not CompleteRunActivity), or "" for a run that
// completed. A step that did not finish, or whose effects were withheld for the owner, must not
// read as a completed run (decision D-f10-4: assumed, owner to confirm). The code is a closed
// word, never content.
func scheduledAgentFailureReason(out activities.StepLoopOutput) string {
	outcome, _, _ := scheduledAgentOutcome(out)
	if outcome == "completed" {
		return ""
	}
	return "scheduled_agent_" + outcome
}

// executeScheduledStepLoop is the schedule-only driver. It never constructs a
// user-bound ExecuteStep request; each turn carries only the immutable,
// template-bound identifiers needed by Control, Session Core, and the
// dedicated Execution Core service lane.
func executeScheduledStepLoop(ctx workflow.Context, in ScheduledRunInput) (activities.StepLoopOutput, error) {
	var steps []activities.StepResult
	for i := range defaultMaxTurns {
		intent := scheduledStepIntent(in, i)
		var step activities.StepResult
		if err := workflow.ExecuteActivity(ctx, "ExecuteScheduledStepActivity", intent).Get(ctx, &step); err != nil {
			return activities.StepLoopOutput{Steps: steps, Completed: false}, err
		}
		steps = append(steps, step)
		if step.UnknownOutcome {
			return activities.StepLoopOutput{Steps: steps, Completed: false}, temporal.NewNonRetryableApplicationError(
				"scheduled-step provider outcome is unknown; reconcile its durable receipt before retrying",
				"ScheduledStepUnknownOutcome",
				nil,
			)
		}
		if step.Completed || step.NeedsApproval {
			break
		}
	}
	return activities.StepLoopOutput{
		Steps:     steps,
		Completed: len(steps) > 0 && steps[len(steps)-1].Completed,
		Summary:   fmt.Sprintf("executed %d scheduled steps for run %s", len(steps), in.RunID),
	}, nil
}

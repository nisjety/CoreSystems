package workflows

import (
	"fmt"

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

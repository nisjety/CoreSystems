package workflows

import (
	"fmt"
	"strings"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/evaloptimizer"
)

// EvaluatorOptimizerInput configures an evaluator-optimizer run. It is the same
// shape the activity consumes; aliasing avoids duplicating the ~15 config
// fields and keeps the workflow and activity contracts in lockstep.
type EvaluatorOptimizerInput = activities.EvalOptimizerInput

// EvaluatorOptimizerResult is the workflow outcome: whether the rubric passed,
// why the loop stopped, and the best attempt.
type EvaluatorOptimizerResult struct {
	Passed      bool    `json:"passed"`
	StopReason  string  `json:"stop_reason"`
	BestAnswer  string  `json:"best_answer"`
	BestScore   float64 `json:"best_score"`
	RoundsRun   int     `json:"rounds_run"`
	TotalTokens int     `json:"total_tokens"`
	Summary     string  `json:"summary"`
}

// evalOptimizerActivityTimeout bounds a single loop run. The loop is itself
// bounded (turn + token caps), but a generous ceiling covers multi-round runs
// against slow providers.
const evalOptimizerActivityTimeout = 10 * time.Minute

// EvaluatorOptimizerWorkflow is the durable envelope for the evaluator-optimizer
// pattern, selectable alongside the other orchestration modes on the
// orchestrator task queue. It runs the bounded generator→judge loop as a single
// retryable activity, then records run completion (or failure) through the same
// lifecycle activities the other patterns use.
//
// A did-not-pass outcome (cap or budget reached) is a CLEAN completion, not a
// workflow failure: the result carries Passed=false and the terminal
// StopReason so callers can observe it. Only an infrastructure/activity error
// fails the workflow and triggers compensation.
func EvaluatorOptimizerWorkflow(ctx workflow.Context, input EvaluatorOptimizerInput) (EvaluatorOptimizerResult, error) {
	logger := workflow.GetLogger(ctx)
	logger.Info("EvaluatorOptimizerWorkflow started",
		"run_id", input.RunID,
		"generator_model", input.GeneratorModel,
		"judge_model", input.JudgeModel,
		"max_rounds", input.MaxRounds,
	)

	if strings.TrimSpace(input.Task) == "" {
		return EvaluatorOptimizerResult{StopReason: "invalid", Summary: "no task provided"}, nil
	}

	activityOpts := workflow.ActivityOptions{
		StartToCloseTimeout: evalOptimizerActivityTimeout,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumAttempts:    3,
		},
	}
	actCtx := workflow.WithActivityOptions(ctx, activityOpts)

	var out activities.EvalOptimizerOutput
	err := workflow.ExecuteActivity(actCtx, "EvaluatorOptimizerActivity", input).Get(ctx, &out)
	if err != nil {
		return EvaluatorOptimizerResult{StopReason: evaloptimizer.StopError, Summary: fmt.Sprintf("evaluator-optimizer failed: %v", err)},
			handleFailure(ctx, input.RunID, input.OrgID, input.UserID, fmt.Sprintf("evaluator-optimizer: %v", err))
	}

	summary := fmt.Sprintf("evaluator-optimizer stopped (%s) after %d round(s): passed=%v, best score %.2f",
		out.StopReason, out.RoundsRun, out.Passed, out.BestScore)

	completionInput := activities.CompletionInput{
		RunID:   input.RunID,
		OrgID:   input.OrgID,
		UserID:  input.UserID,
		Summary: summary,
	}
	if cerr := workflow.ExecuteActivity(actCtx, "CompleteRunActivity", completionInput).Get(ctx, nil); cerr != nil {
		return EvaluatorOptimizerResult{StopReason: out.StopReason, Summary: summary},
			handleFailure(ctx, input.RunID, input.OrgID, input.UserID, fmt.Sprintf("complete run: %v", cerr))
	}

	logger.Info("EvaluatorOptimizerWorkflow completed",
		"run_id", input.RunID,
		"passed", out.Passed,
		"stop_reason", out.StopReason,
		"rounds_run", out.RoundsRun,
		"total_tokens", out.TotalTokens,
	)

	return EvaluatorOptimizerResult{
		Passed:      out.Passed,
		StopReason:  out.StopReason,
		BestAnswer:  out.BestAnswer,
		BestScore:   out.BestScore,
		RoundsRun:   out.RoundsRun,
		TotalTokens: out.TotalTokens,
		Summary:     summary,
	}, nil
}

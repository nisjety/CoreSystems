package workflows

import (
	"fmt"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/evaloptimizer"
)

// EvaluatorOptimizerInput configures an evaluator-optimizer run. It is the same
// shape the activity layer consumes; aliasing avoids duplicating the ~15 config
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

// evalLegActivityTimeout bounds ONE model call (generator or judge leg), not
// the whole loop — each leg is its own activity.
const evalLegActivityTimeout = 3 * time.Minute

// EvaluatorOptimizerWorkflow is the durable, RESUME-SAFE driver for the
// evaluator-optimizer pattern, selectable alongside the other orchestration
// modes on the orchestrator task queue.
//
// Loop engineering: the workflow — not an activity — owns the loop control
// flow. Every generator leg and every judge leg runs as its own
// InferModelActivity, so each completed leg is a durable checkpoint in
// Temporal event history. If the worker crashes mid-loop, replay re-executes
// this (deterministic) workflow code against the recorded leg results and
// resumes at the exact leg that was interrupted — completed rounds are never
// re-run and their tokens never re-spent. Leg request IDs are deterministic
// (run/seq), so a retried leg can also be deduped upstream by inference-core.
//
// The loop logic itself comes from evaloptimizer's exported stepping
// primitives — the same primitives RunLoop uses — so the durable path and the
// in-process path cannot diverge (see evaloptimizer/stepper_test.go).
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

	// Deterministic config assembly — same mapping + defaults as the
	// in-process driver.
	cfg := input.ToConfig().WithDefaults()
	if err := cfg.Validate(); err != nil {
		return EvaluatorOptimizerResult{StopReason: "invalid", Summary: err.Error()}, nil
	}

	activityOpts := workflow.ActivityOptions{
		StartToCloseTimeout: evalLegActivityTimeout,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumAttempts:    3,
		},
	}
	actCtx := workflow.WithActivityOptions(ctx, activityOpts)

	var (
		rounds []evaloptimizer.Attempt
		spent  int
		prev   *evaloptimizer.Attempt
	)
	stop := evaloptimizer.StopMaxRounds

	for round := 0; round < cfg.MaxRounds; round++ {
		// Hard budget stop before spending on a new round (cost-core >= gate).
		if cfg.BudgetExceeded(spent) {
			stop = evaloptimizer.StopBudgetExhausted
			break
		}

		// --- Generator leg (durable checkpoint) ---
		var genRes evaloptimizer.InvokeResult
		err := workflow.ExecuteActivity(actCtx, "InferModelActivity", activities.InferInput{
			RunID: input.RunID,
			Seq:   round * 2,
			Req:   cfg.GeneratorRequest(prev),
		}).Get(ctx, &genRes)
		if err != nil {
			return EvaluatorOptimizerResult{
					StopReason: evaloptimizer.StopError,
					RoundsRun:  len(rounds),
					Summary:    fmt.Sprintf("generator leg failed at round %d: %v", round, err),
				},
				handleFailure(ctx, input.RunID, input.OrgID, input.UserID, fmt.Sprintf("evaluator-optimizer generator round %d: %v", round, err))
		}
		spent += genRes.TotalTokens()

		// --- Judge leg (distinct invocation, durable checkpoint) ---
		var judgeRes evaloptimizer.InvokeResult
		err = workflow.ExecuteActivity(actCtx, "InferModelActivity", activities.InferInput{
			RunID: input.RunID,
			Seq:   round*2 + 1,
			Req:   cfg.JudgeRequest(genRes.Content),
		}).Get(ctx, &judgeRes)
		if err != nil {
			return EvaluatorOptimizerResult{
					StopReason: evaloptimizer.StopError,
					RoundsRun:  len(rounds),
					Summary:    fmt.Sprintf("judge leg failed at round %d: %v", round, err),
				},
				handleFailure(ctx, input.RunID, input.OrgID, input.UserID, fmt.Sprintf("evaluator-optimizer judge round %d: %v", round, err))
		}
		spent += judgeRes.TotalTokens()

		attempt := evaloptimizer.BuildAttempt(round, genRes, judgeRes)
		rounds = append(rounds, attempt)

		// Per-round observability; best-effort — never fails the loop.
		if perr := workflow.ExecuteActivity(actCtx, "PublishEvalRoundActivity", activities.EvalRoundEvent{
			RunID:    input.RunID,
			OrgID:    input.OrgID,
			UserID:   input.UserID,
			Round:    attempt.Round,
			Passed:   attempt.Verdict.Passed,
			Score:    attempt.Verdict.Score,
			Feedback: attempt.Verdict.Feedback,
		}).Get(ctx, nil); perr != nil {
			logger.Warn("eval round publish failed", "round", round, "error", perr)
		}

		if cfg.RoundPassed(attempt) {
			stop = evaloptimizer.StopPassed
			break
		}

		a := attempt // avoid aliasing the loop variable
		prev = &a
	}

	outcome := evaloptimizer.BuildOutcome(rounds, stop, spent)
	summary := fmt.Sprintf("evaluator-optimizer stopped (%s) after %d round(s): passed=%v, best score %.2f",
		outcome.StopReason, outcome.RoundsRun(), outcome.Passed, outcome.Best.Verdict.Score)

	// Terminal observability (counters + outcome event); best-effort.
	if rerr := workflow.ExecuteActivity(actCtx, "RecordEvalOutcomeActivity", activities.EvalOutcomeEvent{
		RunID:       input.RunID,
		OrgID:       input.OrgID,
		UserID:      input.UserID,
		StopReason:  outcome.StopReason,
		Passed:      outcome.Passed,
		RoundsRun:   outcome.RoundsRun(),
		TotalTokens: outcome.TotalTokens,
		BestScore:   outcome.Best.Verdict.Score,
	}).Get(ctx, nil); rerr != nil {
		logger.Warn("eval outcome record failed", "error", rerr)
	}

	completionInput := activities.CompletionInput{
		RunID:   input.RunID,
		OrgID:   input.OrgID,
		UserID:  input.UserID,
		Summary: summary,
	}
	if cerr := workflow.ExecuteActivity(actCtx, "CompleteRunActivity", completionInput).Get(ctx, nil); cerr != nil {
		return EvaluatorOptimizerResult{StopReason: outcome.StopReason, Summary: summary},
			handleFailure(ctx, input.RunID, input.OrgID, input.UserID, fmt.Sprintf("complete run: %v", cerr))
	}

	logger.Info("EvaluatorOptimizerWorkflow completed",
		"run_id", input.RunID,
		"passed", outcome.Passed,
		"stop_reason", outcome.StopReason,
		"rounds_run", outcome.RoundsRun(),
		"total_tokens", outcome.TotalTokens,
	)

	return EvaluatorOptimizerResult{
		Passed:      outcome.Passed,
		StopReason:  outcome.StopReason,
		BestAnswer:  outcome.Best.Answer,
		BestScore:   outcome.Best.Verdict.Score,
		RoundsRun:   outcome.RoundsRun(),
		TotalTokens: outcome.TotalTokens,
		Summary:     summary,
	}, nil
}

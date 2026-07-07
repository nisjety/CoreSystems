package workflows

import (
	"fmt"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// AutoresearchConfig is the input for the autoresearch workflow.
type AutoresearchConfig struct {
	Hypothesis     string  `json:"hypothesis"`
	MaxIterations  int     `json:"max_iterations"`
	BudgetUSD      float64 `json:"budget_usd"`
	TimeoutMinutes int     `json:"timeout_minutes"`
	OrgID          string  `json:"org_id"`
	UserID         string  `json:"user_id"`
	RunID          string  `json:"run_id"`
}

// ProgramArtifact captures the outcome of a single experiment iteration.
type ProgramArtifact struct {
	Iteration    int     `json:"iteration"`
	Hypothesis   string  `json:"hypothesis"`
	Result       string  `json:"result"`
	Decision     string  `json:"decision"` // "keep" or "discard"
	CostEstimate float64 `json:"cost_estimate"`
}

// AutoresearchReport is the final output of the autoresearch workflow.
type AutoresearchReport struct {
	KeptArtifacts  []ProgramArtifact `json:"kept_artifacts"`
	DiscardedCount int               `json:"discarded_count"`
	TotalIter      int               `json:"total_iterations"`
	TotalCostUSD   float64           `json:"total_cost_usd"`
	Summary        string            `json:"summary"`
}

// experimentPlanOutput is the result of the plan-generation activity.
type experimentPlanOutput struct {
	Plan         string  `json:"plan"`
	CostEstimate float64 `json:"cost_estimate"`
}

// experimentEvalOutput is the result of the evaluation activity.
type experimentEvalOutput struct {
	Decision string `json:"decision"` // "keep" or "discard"
	Reason   string `json:"reason"`
}

// AutoresearchWorkflow runs a bounded experiment loop that generates plans,
// executes experiments, evaluates results, and decides whether to keep or
// discard each artifact. The loop stops when max iterations are reached or
// the cumulative cost estimate exceeds the budget.
func AutoresearchWorkflow(ctx workflow.Context, cfg AutoresearchConfig) (AutoresearchReport, error) {
	logger := workflow.GetLogger(ctx)
	logger.Info("AutoresearchWorkflow started",
		"hypothesis", cfg.Hypothesis,
		"max_iterations", cfg.MaxIterations,
		"budget_usd", cfg.BudgetUSD,
		"run_id", cfg.RunID,
	)

	if cfg.Hypothesis == "" {
		return AutoresearchReport{Summary: "no hypothesis provided"}, nil
	}
	if cfg.MaxIterations <= 0 {
		cfg.MaxIterations = 5
	}
	if cfg.BudgetUSD <= 0 {
		cfg.BudgetUSD = 10.0
	}
	if cfg.TimeoutMinutes <= 0 {
		cfg.TimeoutMinutes = 30
	}

	timeout := time.Duration(cfg.TimeoutMinutes) * time.Minute
	activityOpts := workflow.ActivityOptions{
		StartToCloseTimeout: timeout,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumAttempts:    3,
		},
	}
	actCtx := workflow.WithActivityOptions(ctx, activityOpts)

	var (
		artifacts      []ProgramArtifact
		keptArtifacts  []ProgramArtifact
		discardedCount int
		totalCost      float64
	)

	for i := 0; i < cfg.MaxIterations; i++ {
		// --- Phase 1: Generate experiment plan ---
		planInput := activities.StepLoopInput{
			RunID:    fmt.Sprintf("%s-plan-%d", cfg.RunID, i),
			ThreadID: cfg.RunID,
			Goal:     fmt.Sprintf("generate experiment plan for hypothesis: %s (iteration %d)", cfg.Hypothesis, i),
			Policy:   "research-plan",
			MaxTurns: 3,
			OrgID:    cfg.OrgID,
			UserID:   cfg.UserID,
		}

		var planOutput activities.StepLoopOutput
		err := workflow.ExecuteActivity(actCtx,
			"ExecuteStepLoopActivity",
			planInput,
		).Get(ctx, &planOutput)
		if err != nil {
			return buildReport(keptArtifacts, discardedCount, i, totalCost,
				fmt.Sprintf("plan generation failed at iteration %d: %v", i, err),
			), fmt.Errorf("autoresearch plan iteration %d: %w", i, err)
		}

		plan := experimentPlanOutput{
			Plan:         planOutput.Summary,
			CostEstimate: estimateCost(planOutput),
		}

		// Budget check before executing the experiment.
		if totalCost+plan.CostEstimate > cfg.BudgetUSD {
			logger.Info("budget would be exceeded, stopping",
				"iteration", i,
				"total_cost", totalCost,
				"next_estimate", plan.CostEstimate,
				"budget", cfg.BudgetUSD,
			)
			break
		}

		// --- Phase 2: Execute experiment ---
		execInput := activities.StepLoopInput{
			RunID:    fmt.Sprintf("%s-exec-%d", cfg.RunID, i),
			ThreadID: cfg.RunID,
			Goal:     fmt.Sprintf("execute experiment: %s", plan.Plan),
			Policy:   "research-exec",
			MaxTurns: 5,
			OrgID:    cfg.OrgID,
			UserID:   cfg.UserID,
		}

		var execOutput activities.StepLoopOutput
		err = workflow.ExecuteActivity(actCtx,
			"ExecuteStepLoopActivity",
			execInput,
		).Get(ctx, &execOutput)
		if err != nil {
			return buildReport(keptArtifacts, discardedCount, i, totalCost,
				fmt.Sprintf("experiment execution failed at iteration %d: %v", i, err),
			), fmt.Errorf("autoresearch exec iteration %d: %w", i, err)
		}

		// --- Phase 3: Evaluate result ---
		evalInput := activities.StepLoopInput{
			RunID:    fmt.Sprintf("%s-eval-%d", cfg.RunID, i),
			ThreadID: cfg.RunID,
			Goal:     fmt.Sprintf("evaluate experiment result: %s", execOutput.Summary),
			Policy:   "research-eval",
			MaxTurns: 3,
			OrgID:    cfg.OrgID,
			UserID:   cfg.UserID,
		}

		var evalOutput activities.StepLoopOutput
		err = workflow.ExecuteActivity(actCtx,
			"ExecuteStepLoopActivity",
			evalInput,
		).Get(ctx, &evalOutput)
		if err != nil {
			return buildReport(keptArtifacts, discardedCount, i, totalCost,
				fmt.Sprintf("evaluation failed at iteration %d: %v", i, err),
			), fmt.Errorf("autoresearch eval iteration %d: %w", i, err)
		}

		eval := parseEvalDecision(evalOutput)

		// Accumulate cost after successful execution.
		totalCost += plan.CostEstimate

		artifact := ProgramArtifact{
			Iteration:    i,
			Hypothesis:   cfg.Hypothesis,
			Result:       execOutput.Summary,
			Decision:     eval.Decision,
			CostEstimate: plan.CostEstimate,
		}
		artifacts = append(artifacts, artifact)

		if eval.Decision == "keep" {
			keptArtifacts = append(keptArtifacts, artifact)
		} else {
			discardedCount++
		}

		logger.Info("iteration completed",
			"iteration", i,
			"decision", eval.Decision,
			"total_cost", totalCost,
		)
	}

	report := buildReport(keptArtifacts, discardedCount, len(artifacts), totalCost,
		fmt.Sprintf("completed %d iterations for hypothesis %q: %d kept, %d discarded, $%.2f spent",
			len(artifacts), cfg.Hypothesis, len(keptArtifacts), discardedCount, totalCost),
	)

	logger.Info("AutoresearchWorkflow completed",
		"total_iterations", report.TotalIter,
		"kept", len(report.KeptArtifacts),
		"discarded", report.DiscardedCount,
		"total_cost", report.TotalCostUSD,
	)

	return report, nil
}

// buildReport constructs an AutoresearchReport from accumulated state.
func buildReport(kept []ProgramArtifact, discarded, totalIter int, totalCost float64, summary string) AutoresearchReport {
	return AutoresearchReport{
		KeptArtifacts:  kept,
		DiscardedCount: discarded,
		TotalIter:      totalIter,
		TotalCostUSD:   totalCost,
		Summary:        summary,
	}
}

// estimateCost derives a cost estimate from a step loop output.
// In a real system this would parse structured cost data from the activity.
// Here we use the number of steps as a proxy: each step costs $0.10.
func estimateCost(output activities.StepLoopOutput) float64 {
	steps := len(output.Steps)
	if steps == 0 {
		steps = 1
	}
	return float64(steps) * 0.10
}

// parseEvalDecision extracts a keep/discard decision from an evaluation step
// loop output. If the summary contains "keep" (case-insensitive), the decision
// is "keep"; otherwise "discard".
func parseEvalDecision(output activities.StepLoopOutput) experimentEvalOutput {
	decision := "discard"
	for _, step := range output.Steps {
		if step.Completed {
			decision = "keep"
			break
		}
	}
	return experimentEvalOutput{
		Decision: decision,
		Reason:   output.Summary,
	}
}

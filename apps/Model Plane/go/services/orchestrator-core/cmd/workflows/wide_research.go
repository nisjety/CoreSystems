package workflows

import (
	"fmt"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// WideResearchInput is the input for the wide research workflow.
type WideResearchInput struct {
	Queries       []string `json:"queries"`
	MaxBranches   int      `json:"max_branches"`
	MergeStrategy string   `json:"merge_strategy"` // "concat", "dedupe", "summarize"
}

// ResearchBranchResult is the output from a single research branch.
type ResearchBranchResult struct {
	Query  string   `json:"query"`
	Output string   `json:"output"`
	Facts  []string `json:"facts,omitempty"`
	Error  string   `json:"error,omitempty"`
}

// WideResearchOutput collects and merges all research branch results.
type WideResearchOutput struct {
	BranchResults []ResearchBranchResult `json:"branch_results"`
	MergedSummary string                 `json:"merged_summary"`
	UniqueFactCount int                  `json:"unique_fact_count"`
}

// WideResearchWorkflow fans-out parallel research queries, then fans-in results.
// Uses workflow.NewSelector for bounded concurrency.
func WideResearchWorkflow(ctx workflow.Context, input WideResearchInput) (WideResearchOutput, error) {
	logger := workflow.GetLogger(ctx)
	logger.Info("WideResearchWorkflow started",
		"queries", len(input.Queries),
		"max_branches", input.MaxBranches,
	)

	if len(input.Queries) == 0 {
		return WideResearchOutput{MergedSummary: "no queries provided"}, nil
	}

	if input.MaxBranches <= 0 {
		input.MaxBranches = len(input.Queries)
	}
	if input.MaxBranches > len(input.Queries) {
		input.MaxBranches = len(input.Queries)
	}

	if input.MergeStrategy == "" {
		input.MergeStrategy = "dedupe"
	}

	activityOpts := workflow.ActivityOptions{
		StartToCloseTimeout: 5 * time.Minute,
		RetryPolicy: &temporal.RetryPolicy{
			InitialInterval:    time.Second,
			BackoffCoefficient: 2.0,
			MaximumAttempts:    3,
		},
	}
	actCtx := workflow.WithActivityOptions(ctx, activityOpts)

	// Fan-out: spawn parallel research branches with bounded concurrency.
	results := make([]ResearchBranchResult, len(input.Queries))
	selector := workflow.NewSelector(ctx)
	inflight := 0

	for i, query := range input.Queries {
		idx := i
		q := query

		stepInput := activities.StepLoopInput{
			RunID:    fmt.Sprintf("research-branch-%d", idx),
			ThreadID: "wide-research",
			Goal:     q,
			Policy:   "research",
			MaxTurns: 5,
		}

		future := workflow.ExecuteActivity(actCtx,
			"ExecuteStepLoopActivity",
			stepInput,
		)

		selector.AddFuture(future, func(f workflow.Future) {
			var output activities.StepLoopOutput
			err := f.Get(ctx, &output)

			result := ResearchBranchResult{Query: q}
			if err != nil {
				result.Error = err.Error()
			} else {
				result.Output = output.Summary
				for _, step := range output.Steps {
					if step.Output != "" {
						result.Facts = append(result.Facts, step.Output)
					}
				}
			}
			results[idx] = result
		})

		inflight++

		// Bounded concurrency: drain when at capacity.
		if inflight >= input.MaxBranches {
			selector.Select(ctx)
			inflight--
		}
	}

	// Drain remaining futures.
	for inflight > 0 {
		selector.Select(ctx)
		inflight--
	}

	// Fan-in: merge results.
	output := mergeResults(results, input.MergeStrategy)

	logger.Info("WideResearchWorkflow completed",
		"branches", len(results),
		"unique_facts", output.UniqueFactCount,
	)

	return output, nil
}

// mergeResults combines branch results using the specified strategy.
func mergeResults(results []ResearchBranchResult, strategy string) WideResearchOutput {
	seen := make(map[string]struct{})
	var allFacts []string

	for _, r := range results {
		for _, fact := range r.Facts {
			if _, exists := seen[fact]; !exists {
				seen[fact] = struct{}{}
				allFacts = append(allFacts, fact)
			}
		}
	}

	var summary string
	switch strategy {
	case "concat":
		for _, r := range results {
			summary += fmt.Sprintf("[%s]: %s\n", r.Query, r.Output)
		}
	case "summarize":
		summary = fmt.Sprintf("synthesized %d branches with %d unique facts", len(results), len(allFacts))
	default: // "dedupe"
		summary = fmt.Sprintf("deduplicated results from %d branches: %d unique facts", len(results), len(allFacts))
	}

	return WideResearchOutput{
		BranchResults:   results,
		MergedSummary:   summary,
		UniqueFactCount: len(allFacts),
	}
}

package workflows

import (
	"fmt"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// TaskStep describes a single step in a deep task workflow.
type TaskStep struct {
	Goal    string `json:"goal"`
	AgentID string `json:"agent_id"`
	Policy  string `json:"policy"`
}

// DeepTaskInput is the input for the deep task workflow.
type DeepTaskInput struct {
	ParentRunID string     `json:"parent_run_id"`
	Steps       []TaskStep `json:"steps"`
	OrgID       string     `json:"org_id"`
	UserID      string     `json:"user_id"`
}

// DeepTaskOutput collects the outputs from all sequential steps.
type DeepTaskOutput struct {
	StepOutputs []activities.StepLoopOutput `json:"step_outputs"`
	Summary     string                      `json:"summary"`
}

// DeepTaskWorkflow executes complex tasks that need multiple sequential subagent runs.
// Each step is executed in order, and the results fan-in to a final summary.
func DeepTaskWorkflow(ctx workflow.Context, input DeepTaskInput) (DeepTaskOutput, error) {
	logger := workflow.GetLogger(ctx)
	logger.Info("DeepTaskWorkflow started",
		"parent_run_id", input.ParentRunID,
		"step_count", len(input.Steps),
	)

	if len(input.Steps) == 0 {
		return DeepTaskOutput{Summary: "no steps to execute"}, nil
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

	var stepOutputs []activities.StepLoopOutput

	for i, step := range input.Steps {
		logger.Info("executing deep task step",
			"parent_run_id", input.ParentRunID,
			"step_index", i,
			"goal", step.Goal,
			"agent_id", step.AgentID,
		)

		stepInput := activities.StepLoopInput{
			RunID:    fmt.Sprintf("%s-step-%d", input.ParentRunID, i),
			ThreadID: input.ParentRunID,
			Goal:     step.Goal,
			Policy:   step.Policy,
			MaxTurns: defaultMaxTurns,
			OrgID:    input.OrgID,
			UserID:   input.UserID,
		}

		var output activities.StepLoopOutput
		err := workflow.ExecuteActivity(actCtx,
			"ExecuteStepLoopActivity",
			stepInput,
		).Get(ctx, &output)
		if err != nil {
			return DeepTaskOutput{
				StepOutputs: stepOutputs,
				Summary:     fmt.Sprintf("failed at step %d: %v", i, err),
			}, fmt.Errorf("deep task step %d failed: %w", i, err)
		}

		stepOutputs = append(stepOutputs, output)
	}

	summary := fmt.Sprintf("completed %d/%d steps for parent run %s",
		len(stepOutputs), len(input.Steps), input.ParentRunID)

	logger.Info("DeepTaskWorkflow completed", "parent_run_id", input.ParentRunID, "summary", summary)

	return DeepTaskOutput{
		StepOutputs: stepOutputs,
		Summary:     summary,
	}, nil
}

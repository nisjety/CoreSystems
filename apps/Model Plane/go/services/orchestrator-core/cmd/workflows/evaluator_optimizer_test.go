package workflows

import (
	"errors"
	"io"
	"log/slog"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/activity"
	"go.temporal.io/sdk/testsuite"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// newEvalOptimizerEnv builds a Temporal test environment with the three
// activities the workflow drives registered by name. EvaluatorOptimizerActivity
// is registered so string lookups resolve; individual tests override it with a
// mock so no gRPC/inference-core is required.
func newEvalOptimizerEnv(t *testing.T) *testsuite.TestWorkflowEnvironment {
	t.Helper()
	var suite testsuite.WorkflowTestSuite
	env := suite.NewTestWorkflowEnvironment()

	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	a := activities.NewActivities(logger, nil)
	env.RegisterActivityWithOptions(a.EvaluatorOptimizerActivity, activity.RegisterOptions{Name: "EvaluatorOptimizerActivity"})
	env.RegisterActivityWithOptions(a.CompleteRunActivity, activity.RegisterOptions{Name: "CompleteRunActivity"})
	env.RegisterActivityWithOptions(a.FailRunActivity, activity.RegisterOptions{Name: "FailRunActivity"})

	return env
}

func baseWorkflowInput() EvaluatorOptimizerInput {
	return EvaluatorOptimizerInput{
		RunID:          "run-eo-wf",
		ThreadID:       "thread-eo",
		OrgID:          "org-1",
		UserID:         "user-1",
		GeneratorModel: "claude-sonnet-5",
		Task:           "write a tagline",
		JudgeModel:     "claude-haiku-4-5",
		Rubric:         "punchy, under 8 words",
		MaxRounds:      3,
	}
}

func TestEvaluatorOptimizerWorkflow(t *testing.T) {
	tests := []struct {
		name            string
		input           EvaluatorOptimizerInput
		mockOut         activities.EvalOptimizerOutput
		mockErr         error
		mockActivity    bool
		wantWorkflowErr bool
		wantPassed      bool
		wantStopReason  string
		wantSummaryHas  string
	}{
		{
			name:           "passes on first round",
			input:          baseWorkflowInput(),
			mockActivity:   true,
			mockOut:        activities.EvalOptimizerOutput{Passed: true, StopReason: "passed", BestAnswer: "Ship it.", BestScore: 0.93, RoundsRun: 1, TotalTokens: 42},
			wantPassed:     true,
			wantStopReason: "passed",
			wantSummaryHas: "passed=true",
		},
		{
			name:           "did-not-pass at cap is a clean completion",
			input:          baseWorkflowInput(),
			mockActivity:   true,
			mockOut:        activities.EvalOptimizerOutput{Passed: false, StopReason: "max_rounds", BestAnswer: "meh", BestScore: 0.6, RoundsRun: 3, TotalTokens: 100},
			wantPassed:     false,
			wantStopReason: "max_rounds",
			wantSummaryHas: "max_rounds",
		},
		{
			name:           "budget exhaustion is a clean completion",
			input:          baseWorkflowInput(),
			mockActivity:   true,
			mockOut:        activities.EvalOptimizerOutput{Passed: false, StopReason: "budget_exhausted", BestAnswer: "partial", BestScore: 0.5, RoundsRun: 2, TotalTokens: 500},
			wantPassed:     false,
			wantStopReason: "budget_exhausted",
			wantSummaryHas: "budget_exhausted",
		},
		{
			name: "empty task returns early without invoking the activity",
			input: func() EvaluatorOptimizerInput {
				in := baseWorkflowInput()
				in.Task = ""
				return in
			}(),
			mockActivity:   false,
			wantPassed:     false,
			wantStopReason: "invalid",
			wantSummaryHas: "no task",
		},
		{
			name:            "activity error fails the workflow",
			input:           baseWorkflowInput(),
			mockActivity:    true,
			mockErr:         errors.New("inference-core unavailable"),
			wantWorkflowErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			env := newEvalOptimizerEnv(t)
			if tt.mockActivity {
				call := env.OnActivity("EvaluatorOptimizerActivity", mock.Anything, mock.Anything)
				if tt.mockErr != nil {
					call.Return(activities.EvalOptimizerOutput{}, tt.mockErr)
				} else {
					call.Return(tt.mockOut, nil)
				}
			}

			env.ExecuteWorkflow(EvaluatorOptimizerWorkflow, tt.input)

			require.True(t, env.IsWorkflowCompleted(), "workflow should complete")

			if tt.wantWorkflowErr {
				require.Error(t, env.GetWorkflowError())
				return
			}
			require.NoError(t, env.GetWorkflowError())

			var result EvaluatorOptimizerResult
			require.NoError(t, env.GetWorkflowResult(&result))

			assert.Equal(t, tt.wantPassed, result.Passed, "passed")
			assert.Equal(t, tt.wantStopReason, result.StopReason, "stop reason")
			assert.Contains(t, result.Summary, tt.wantSummaryHas, "summary")

			if tt.mockActivity {
				assert.Equal(t, tt.mockOut.BestAnswer, result.BestAnswer)
				assert.Equal(t, tt.mockOut.RoundsRun, result.RoundsRun)
			}
		})
	}
}

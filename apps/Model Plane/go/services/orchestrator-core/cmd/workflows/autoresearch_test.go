package workflows

import (
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

// newAutoresearchEnv builds a Temporal test workflow environment configured
// for the AutoresearchWorkflow. It registers the ExecuteStepLoopActivity
// by name so string-based lookups resolve correctly.
func newAutoresearchEnv(t *testing.T) *testsuite.TestWorkflowEnvironment {
	t.Helper()
	var suite testsuite.WorkflowTestSuite
	env := suite.NewTestWorkflowEnvironment()

	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	a := activities.NewActivities(logger, nil)
	env.RegisterActivityWithOptions(a.ExecuteStepLoopActivity, activity.RegisterOptions{Name: "ExecuteStepLoopActivity"})

	return env
}

// stepOutput builds an activities.StepLoopOutput with N steps.
// If completed is true the last step is marked Completed.
func stepOutput(nSteps int, completed bool, summary string) activities.StepLoopOutput {
	steps := make([]activities.StepResult, nSteps)
	for i := range steps {
		steps[i] = activities.StepResult{StepIndex: i, ToolName: "stub"}
	}
	if completed && len(steps) > 0 {
		steps[len(steps)-1].Completed = true
	}
	return activities.StepLoopOutput{
		Steps:     steps,
		Completed: completed,
		Summary:   summary,
	}
}

func TestAutoresearchWorkflow(t *testing.T) {
	tests := []struct {
		name           string
		cfg            AutoresearchConfig
		setupMocks     func(env *testsuite.TestWorkflowEnvironment)
		wantErr        bool
		wantKept       int
		wantDiscarded  int
		wantIter       int
		wantMaxCostUSD float64
		wantSummaryHas string
	}{
		{
			name: "normal completion after 3 iterations with kept and discarded",
			cfg: AutoresearchConfig{
				Hypothesis:     "test hypothesis",
				MaxIterations:  3,
				BudgetUSD:      10.0,
				TimeoutMinutes: 5,
				OrgID:          "org-1",
				RunID:          "run-auto-1",
			},
			setupMocks: func(env *testsuite.TestWorkflowEnvironment) {
				// Each iteration invokes 3 activities: plan, exec, eval.
				// Iteration 0: plan(1 step) + exec(2 steps) + eval(completed=true → keep)
				// Iteration 1: plan(1 step) + exec(2 steps) + eval(completed=false → discard)
				// Iteration 2: plan(1 step) + exec(2 steps) + eval(completed=true → keep)
				call0Plan := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-auto-1-plan-0"
				})).Return(stepOutput(1, true, "plan-0"), nil).Once()

				call0Exec := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-auto-1-exec-0"
				})).Return(stepOutput(2, true, "exec-result-0"), nil).NotBefore(call0Plan).Once()

				call0Eval := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-auto-1-eval-0"
				})).Return(stepOutput(1, true, "keep"), nil).NotBefore(call0Exec).Once()

				call1Plan := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-auto-1-plan-1"
				})).Return(stepOutput(1, true, "plan-1"), nil).NotBefore(call0Eval).Once()

				call1Exec := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-auto-1-exec-1"
				})).Return(stepOutput(2, true, "exec-result-1"), nil).NotBefore(call1Plan).Once()

				call1Eval := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-auto-1-eval-1"
				})).Return(stepOutput(1, false, "discard"), nil).NotBefore(call1Exec).Once()

				call2Plan := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-auto-1-plan-2"
				})).Return(stepOutput(1, true, "plan-2"), nil).NotBefore(call1Eval).Once()

				call2Exec := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-auto-1-exec-2"
				})).Return(stepOutput(2, true, "exec-result-2"), nil).NotBefore(call2Plan).Once()

				env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-auto-1-eval-2"
				})).Return(stepOutput(1, true, "keep"), nil).NotBefore(call2Exec).Once()
			},
			wantErr:        false,
			wantKept:       2,
			wantDiscarded:  1,
			wantIter:       3,
			wantMaxCostUSD: 10.0,
			wantSummaryHas: "completed 3 iterations",
		},
		{
			name: "budget exceeded stops early",
			cfg: AutoresearchConfig{
				Hypothesis:     "expensive hypothesis",
				MaxIterations:  10,
				BudgetUSD:      0.15, // only enough for 1 iteration (plan cost = 0.10)
				TimeoutMinutes: 5,
				OrgID:          "org-2",
				RunID:          "run-budget",
			},
			setupMocks: func(env *testsuite.TestWorkflowEnvironment) {
				// Iteration 0: plan(1 step, cost $0.10) + exec + eval → fits in budget
				call0Plan := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-budget-plan-0"
				})).Return(stepOutput(1, true, "plan-0"), nil).Once()

				call0Exec := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-budget-exec-0"
				})).Return(stepOutput(1, true, "exec-0"), nil).NotBefore(call0Plan).Once()

				call0Eval := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-budget-eval-0"
				})).Return(stepOutput(1, true, "keep"), nil).NotBefore(call0Exec).Once()

				// Iteration 1: plan(1 step, cost $0.10) → totalCost would be $0.20 > $0.15 → stops.
				env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-budget-plan-1"
				})).Return(stepOutput(1, true, "plan-1"), nil).NotBefore(call0Eval).Once()
			},
			wantErr:        false,
			wantKept:       1,
			wantDiscarded:  0,
			wantIter:       1,
			wantMaxCostUSD: 0.15,
			wantSummaryHas: "1 kept",
		},
		{
			name: "max iterations reached",
			cfg: AutoresearchConfig{
				Hypothesis:     "short hypothesis",
				MaxIterations:  1,
				BudgetUSD:      100.0,
				TimeoutMinutes: 5,
				OrgID:          "org-3",
				RunID:          "run-maxiter",
			},
			setupMocks: func(env *testsuite.TestWorkflowEnvironment) {
				callPlan := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-maxiter-plan-0"
				})).Return(stepOutput(1, true, "plan-0"), nil).Once()

				callExec := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-maxiter-exec-0"
				})).Return(stepOutput(1, true, "exec-0"), nil).NotBefore(callPlan).Once()

				env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.MatchedBy(func(in activities.StepLoopInput) bool {
					return in.RunID == "run-maxiter-eval-0"
				})).Return(stepOutput(1, true, "keep"), nil).NotBefore(callExec).Once()
			},
			wantErr:        false,
			wantKept:       1,
			wantDiscarded:  0,
			wantIter:       1,
			wantMaxCostUSD: 100.0,
			wantSummaryHas: "completed 1 iterations",
		},
		{
			name: "empty hypothesis returns early",
			cfg: AutoresearchConfig{
				Hypothesis:     "",
				MaxIterations:  5,
				BudgetUSD:      10.0,
				TimeoutMinutes: 5,
				OrgID:          "org-4",
				RunID:          "run-empty",
			},
			setupMocks:     func(_ *testsuite.TestWorkflowEnvironment) {},
			wantErr:        false,
			wantKept:       0,
			wantDiscarded:  0,
			wantIter:       0,
			wantSummaryHas: "no hypothesis",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			env := newAutoresearchEnv(t)
			tt.setupMocks(env)

			env.ExecuteWorkflow(AutoresearchWorkflow, tt.cfg)

			require.True(t, env.IsWorkflowCompleted(), "workflow should complete")

			if tt.wantErr {
				require.Error(t, env.GetWorkflowError())
				return
			}

			require.NoError(t, env.GetWorkflowError())

			var report AutoresearchReport
			require.NoError(t, env.GetWorkflowResult(&report))

			assert.Equal(t, tt.wantKept, len(report.KeptArtifacts), "kept artifacts count")
			assert.Equal(t, tt.wantDiscarded, report.DiscardedCount, "discarded count")
			assert.Equal(t, tt.wantIter, report.TotalIter, "total iterations")
			assert.LessOrEqual(t, report.TotalCostUSD, tt.wantMaxCostUSD, "cost should not exceed budget")
			assert.Contains(t, report.Summary, tt.wantSummaryHas, "summary content")

			// Verify artifact integrity.
			for _, a := range report.KeptArtifacts {
				assert.Equal(t, "keep", a.Decision)
				assert.Equal(t, tt.cfg.Hypothesis, a.Hypothesis)
				assert.Greater(t, a.CostEstimate, 0.0)
			}
		})
	}
}

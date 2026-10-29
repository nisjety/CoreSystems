package workflows

import (
	"fmt"
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

// newAutoresearchEnv builds a Temporal test workflow environment configured for
// the AutoresearchWorkflow. New runs drive the step loop per turn via
// ExecuteStepActivity; the legacy ExecuteStepLoopActivity stays registered for
// old-history replay. Both are registered by name so string lookups resolve.
func newAutoresearchEnv(t *testing.T) *testsuite.TestWorkflowEnvironment {
	t.Helper()
	var suite testsuite.WorkflowTestSuite
	env := suite.NewTestWorkflowEnvironment()

	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	a := activities.NewActivities(logger, nil)
	env.RegisterActivityWithOptions(a.ExecuteStepLoopActivity, activity.RegisterOptions{Name: "ExecuteStepLoopActivity"})
	env.RegisterActivityWithOptions(a.ExecuteStepActivity, activity.RegisterOptions{Name: "ExecuteStepActivity"})

	return env
}

// completedResult ends a per-turn step loop after its first turn (Completed set
// on turn 0 → a 1-step loop). Used for plan/exec loops — the plan loop's step
// count drives the $0.10/iteration cost estimate — and for eval loops that
// should decide "keep".
func completedResult() activities.StepResult {
	return activities.StepResult{ToolName: "stub", Completed: true}
}

// pendingResult never terminates the loop, so it runs to its MaxTurns cap with
// no Completed turn. Used for eval loops that should decide "discard".
func pendingResult() activities.StepResult {
	return activities.StepResult{ToolName: "stub"}
}

// onStep mocks ExecuteStepActivity for one phase's step loop (matched by RunID)
// to return res on every turn of that loop.
func onStep(env *testsuite.TestWorkflowEnvironment, runID string, res activities.StepResult) {
	env.OnActivity("ExecuteStepActivity", mock.Anything, mock.MatchedBy(func(in activities.StepInput) bool {
		return in.RunID == runID
	})).Return(res, nil)
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
				// Each iteration runs 3 per-turn step loops: plan, exec, eval.
				// plan/exec complete on turn 0 (→ 1 step; plan's step count sets the
				// $0.10 cost). eval decides keep when its loop completes, discard
				// when it runs to the turn cap with no completed turn.
				// Iter 0: eval → keep. Iter 1: eval → discard. Iter 2: eval → keep.
				for _, iter := range []int{0, 1, 2} {
					onStep(env, fmt.Sprintf("run-auto-1-plan-%d", iter), completedResult())
					onStep(env, fmt.Sprintf("run-auto-1-exec-%d", iter), completedResult())
				}
				onStep(env, "run-auto-1-eval-0", completedResult())
				onStep(env, "run-auto-1-eval-1", pendingResult())
				onStep(env, "run-auto-1-eval-2", completedResult())
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
				// Iteration 0: plan(1 step, cost $0.10) + exec + eval → fits in budget.
				onStep(env, "run-budget-plan-0", completedResult())
				onStep(env, "run-budget-exec-0", completedResult())
				onStep(env, "run-budget-eval-0", completedResult()) // keep
				// Iteration 1: plan(1 step, cost $0.10) → total $0.20 > $0.15 → stops
				// before exec-1/eval-1 ever run.
				onStep(env, "run-budget-plan-1", completedResult())
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
				onStep(env, "run-maxiter-plan-0", completedResult())
				onStep(env, "run-maxiter-exec-0", completedResult())
				onStep(env, "run-maxiter-eval-0", completedResult()) // keep
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

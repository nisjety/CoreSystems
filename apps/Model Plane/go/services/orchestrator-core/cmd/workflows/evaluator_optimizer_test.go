package workflows

import (
	"errors"
	"io"
	"log/slog"
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/activity"
	"go.temporal.io/sdk/testsuite"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/evaloptimizer"
)

// newEvalOptimizerEnv builds a Temporal test environment for the durable
// per-leg driver. InferModelActivity is registered by name so string lookups
// resolve; tests script its responses per leg (Seq) with mocks, so no
// gRPC/inference-core is required. The observability + lifecycle activities
// run their real implementations (nil publisher → no-op).
func newEvalOptimizerEnv(t *testing.T) *testsuite.TestWorkflowEnvironment {
	t.Helper()
	var suite testsuite.WorkflowTestSuite
	env := suite.NewTestWorkflowEnvironment()

	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	a := activities.NewActivities(logger, nil)
	env.RegisterActivityWithOptions(a.InferModelActivity, activity.RegisterOptions{Name: "InferModelActivity"})
	env.RegisterActivityWithOptions(a.PublishEvalRoundActivity, activity.RegisterOptions{Name: "PublishEvalRoundActivity"})
	env.RegisterActivityWithOptions(a.RecordEvalOutcomeActivity, activity.RegisterOptions{Name: "RecordEvalOutcomeActivity"})
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

// leg scripts one InferModelActivity response for a given deterministic Seq.
type wfLeg struct {
	seq     int
	content string
	tokens  int
}

// mockLegs wires one mock per scripted leg, matched on the deterministic Seq
// (round*2 = generator, round*2+1 = judge) the workflow stamps on each call.
func mockLegs(env *testsuite.TestWorkflowEnvironment, legs []wfLeg) {
	for _, l := range legs {
		l := l
		env.OnActivity("InferModelActivity", mock.Anything, mock.MatchedBy(func(in activities.InferInput) bool {
			return in.Seq == l.seq
		})).Return(evaloptimizer.InvokeResult{Content: l.content, OutputTokens: l.tokens}, nil).Once()
	}
}

func TestEvaluatorOptimizerWorkflow(t *testing.T) {
	tests := []struct {
		name           string
		input          EvaluatorOptimizerInput
		legs           []wfLeg
		wantPassed     bool
		wantStopReason string
		wantRoundsRun  int
		wantBestAnswer string
		wantSummaryHas string
	}{
		{
			name:  "passes on first round",
			input: baseWorkflowInput(),
			legs: []wfLeg{
				{seq: 0, content: "Ship it.", tokens: 10},
				{seq: 1, content: `{"passed": true, "score": 0.93, "feedback": "great"}`, tokens: 5},
			},
			wantPassed:     true,
			wantStopReason: evaloptimizer.StopPassed,
			wantRoundsRun:  1,
			wantBestAnswer: "Ship it.",
			wantSummaryHas: "passed=true",
		},
		{
			name:  "loops then passes",
			input: baseWorkflowInput(),
			legs: []wfLeg{
				{seq: 0, content: "draft 1", tokens: 10},
				{seq: 1, content: `{"passed": false, "score": 0.4, "feedback": "make it shorter"}`, tokens: 5},
				{seq: 2, content: "draft 2", tokens: 10},
				{seq: 3, content: `{"passed": true, "score": 0.9, "feedback": "good"}`, tokens: 5},
			},
			wantPassed:     true,
			wantStopReason: evaloptimizer.StopPassed,
			wantRoundsRun:  2,
			wantBestAnswer: "draft 2",
			wantSummaryHas: "2 round(s)",
		},
		{
			name: "did-not-pass at cap is a clean completion",
			input: func() EvaluatorOptimizerInput {
				in := baseWorkflowInput()
				in.MaxRounds = 2
				return in
			}(),
			legs: []wfLeg{
				{seq: 0, content: "try 1", tokens: 5},
				{seq: 1, content: `{"passed": false, "score": 0.3, "feedback": "again"}`, tokens: 5},
				{seq: 2, content: "try 2", tokens: 5},
				{seq: 3, content: `{"passed": false, "score": 0.6, "feedback": "closer"}`, tokens: 5},
			},
			wantPassed:     false,
			wantStopReason: evaloptimizer.StopMaxRounds,
			wantRoundsRun:  2,
			wantBestAnswer: "try 2", // 0.6 > 0.3
			wantSummaryHas: evaloptimizer.StopMaxRounds,
		},
		{
			name: "budget exhaustion is a clean completion",
			input: func() EvaluatorOptimizerInput {
				in := baseWorkflowInput()
				in.MaxRounds = 5
				in.TotalTokenBudget = 50
				return in
			}(),
			legs: []wfLeg{
				{seq: 0, content: "expensive draft", tokens: 30},
				{seq: 1, content: `{"passed": false, "score": 0.5, "feedback": "no"}`, tokens: 30},
				// spent 60 >= budget 50 → round 1 never starts (no seq 2/3).
			},
			wantPassed:     false,
			wantStopReason: evaloptimizer.StopBudgetExhausted,
			wantRoundsRun:  1,
			wantBestAnswer: "expensive draft",
			wantSummaryHas: evaloptimizer.StopBudgetExhausted,
		},
		{
			name: "invalid config returns early without invoking the model",
			input: func() EvaluatorOptimizerInput {
				in := baseWorkflowInput()
				in.Task = ""
				return in
			}(),
			legs:           nil,
			wantPassed:     false,
			wantStopReason: "invalid",
			wantRoundsRun:  0,
			wantSummaryHas: "task is required",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			env := newEvalOptimizerEnv(t)
			mockLegs(env, tt.legs)

			env.ExecuteWorkflow(EvaluatorOptimizerWorkflow, tt.input)

			require.True(t, env.IsWorkflowCompleted(), "workflow should complete")
			require.NoError(t, env.GetWorkflowError())

			var result EvaluatorOptimizerResult
			require.NoError(t, env.GetWorkflowResult(&result))

			assert.Equal(t, tt.wantPassed, result.Passed, "passed")
			assert.Equal(t, tt.wantStopReason, result.StopReason, "stop reason")
			assert.Equal(t, tt.wantRoundsRun, result.RoundsRun, "rounds run")
			if tt.wantBestAnswer != "" {
				assert.Equal(t, tt.wantBestAnswer, result.BestAnswer, "best answer")
			}
			assert.Contains(t, result.Summary, tt.wantSummaryHas, "summary")

			// Every scripted leg must have been consumed exactly once — the
			// workflow made exactly the durable checkpoints we expected.
			env.AssertExpectations(t)
		})
	}
}

// The judge's feedback must be threaded into the NEXT generator leg's request
// — asserted at the workflow boundary so the durable driver, not just the pure
// loop, is proven to revise rather than restart.
func TestEvaluatorOptimizerWorkflow_FeedbackThreadedAcrossLegs(t *testing.T) {
	env := newEvalOptimizerEnv(t)
	mockLegs(env, []wfLeg{
		{seq: 0, content: "draft 1", tokens: 5},
		{seq: 1, content: `{"passed": false, "score": 0.4, "feedback": "make it rhyme"}`, tokens: 5},
		{seq: 3, content: `{"passed": true, "score": 0.9, "feedback": "good"}`, tokens: 5},
	})
	// The second generator leg (seq 2) must carry the judge feedback and the
	// prior answer.
	env.OnActivity("InferModelActivity", mock.Anything, mock.MatchedBy(func(in activities.InferInput) bool {
		if in.Seq != 2 {
			return false
		}
		var all strings.Builder
		for _, m := range in.Req.Messages {
			all.WriteString(m.Content)
			all.WriteByte('\n')
		}
		return strings.Contains(all.String(), "make it rhyme") && strings.Contains(all.String(), "draft 1")
	})).Return(evaloptimizer.InvokeResult{Content: "draft 2", OutputTokens: 5}, nil).Once()

	env.ExecuteWorkflow(EvaluatorOptimizerWorkflow, baseWorkflowInput())

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	env.AssertExpectations(t)
}

// ZDR must be stamped on every leg the workflow dispatches.
func TestEvaluatorOptimizerWorkflow_ZDROnEveryLeg(t *testing.T) {
	env := newEvalOptimizerEnv(t)
	in := baseWorkflowInput()
	in.ZDR = true

	env.OnActivity("InferModelActivity", mock.Anything, mock.MatchedBy(func(li activities.InferInput) bool {
		return li.Req.ZDR && li.Req.OrgID == "org-1"
	})).Return(evaloptimizer.InvokeResult{Content: `{"passed": true, "score": 1, "feedback": "ok"}`, OutputTokens: 1}, nil).Twice()

	env.ExecuteWorkflow(EvaluatorOptimizerWorkflow, in)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	env.AssertExpectations(t)
}

// An exhausted leg (activity error after retries) fails the workflow and runs
// the FailRun compensation.
func TestEvaluatorOptimizerWorkflow_LegErrorFailsWorkflow(t *testing.T) {
	env := newEvalOptimizerEnv(t)
	env.OnActivity("InferModelActivity", mock.Anything, mock.Anything).
		Return(evaloptimizer.InvokeResult{}, errors.New("inference-core unavailable"))

	env.ExecuteWorkflow(EvaluatorOptimizerWorkflow, baseWorkflowInput())

	require.True(t, env.IsWorkflowCompleted())
	require.Error(t, env.GetWorkflowError())
}

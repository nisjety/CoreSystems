package workflows

import (
	"io"
	"log/slog"
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/testsuite"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

func newFeedbackEnv() *testsuite.TestWorkflowEnvironment {
	var suite testsuite.WorkflowTestSuite
	env := suite.NewTestWorkflowEnvironment()
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	a := activities.NewActivities(logger, nil)
	env.RegisterActivity(a.AggregateFeedbackActivity)
	env.RegisterActivity(a.ValidateSkillBundleActivity)
	env.RegisterActivity(a.RunPromotionGateActivity)
	env.RegisterWorkflow(SkillPromotionWorkflow)
	return env
}

// TestFeedbackPromotionWorkflow_GateSuccessStillSkipsAtRegistryUpdate replaces
// the old "promotes passing candidate" case. SKILL-2 removed
// UpdateRegistryActivity (it called capability-core's now-removed PromoteSkill
// RPC), so the child SkillPromotionWorkflow's step 3 now always fails
// explicitly (ErrRegistryUpdateNotSupported) even when validation and the
// gate both pass. The parent's existing `err == nil && result.Promoted`
// check already treats that as a skip, not a crash — this pins down that a
// candidate that clears every real check still cannot be promoted today.
func TestFeedbackPromotionWorkflow_GateSuccessStillSkipsAtRegistryUpdate(t *testing.T) {
	env := newFeedbackEnv()

	env.OnActivity("AggregateFeedbackActivity", mock.Anything, mock.Anything).
		Return(activities.FeedbackAggregateOutput{
			Candidates: []activities.SkillPromotionCandidate{
				{SkillID: "cap.skill.summarize", FromScope: "agent", ToScope: "workspace", Good: 9, Total: 10, Score: 0.9},
			},
		}, nil).Once()
	env.OnActivity("ValidateSkillBundleActivity", mock.Anything, mock.Anything).
		Return(activities.SkillValidationOutput{Valid: true}, nil).Once()
	env.OnActivity("RunPromotionGateActivity", mock.Anything, mock.Anything).
		Return(activities.PromotionGateOutput{Passed: true, Checks: []string{"ok"}}, nil).Once()

	env.ExecuteWorkflow(FeedbackPromotionWorkflow, FeedbackPromotionInput{MinSamples: 5, PromoteThreshold: 0.8})

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())

	var out FeedbackPromotionOutput
	require.NoError(t, env.GetWorkflowResult(&out))
	require.Equal(t, 1, out.Evaluated)
	require.Empty(t, out.Promoted)
	require.Equal(t, []string{"cap.skill.summarize"}, out.Skipped)
}

func TestFeedbackPromotionWorkflow_SkipsWhenGateFails(t *testing.T) {
	env := newFeedbackEnv()

	env.OnActivity("AggregateFeedbackActivity", mock.Anything, mock.Anything).
		Return(activities.FeedbackAggregateOutput{
			Candidates: []activities.SkillPromotionCandidate{
				{SkillID: "cap.skill.risky", FromScope: "agent", ToScope: "org", Good: 8, Total: 10, Score: 0.8},
			},
		}, nil).Once()
	env.OnActivity("ValidateSkillBundleActivity", mock.Anything, mock.Anything).
		Return(activities.SkillValidationOutput{Valid: true}, nil).Once()
	// Gate fails → child workflow returns Promoted=false → parent records skip.
	env.OnActivity("RunPromotionGateActivity", mock.Anything, mock.Anything).
		Return(activities.PromotionGateOutput{Passed: false, Checks: []string{"scope_too_broad"}}, nil).Once()

	env.ExecuteWorkflow(FeedbackPromotionWorkflow, FeedbackPromotionInput{MinSamples: 5, PromoteThreshold: 0.8})

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())

	var out FeedbackPromotionOutput
	require.NoError(t, env.GetWorkflowResult(&out))
	require.Equal(t, 1, out.Evaluated)
	require.Empty(t, out.Promoted)
	require.Equal(t, []string{"cap.skill.risky"}, out.Skipped)
}

func TestFeedbackPromotionWorkflow_NoCandidates(t *testing.T) {
	env := newFeedbackEnv()

	env.OnActivity("AggregateFeedbackActivity", mock.Anything, mock.Anything).
		Return(activities.FeedbackAggregateOutput{}, nil).Once()

	env.ExecuteWorkflow(FeedbackPromotionWorkflow, FeedbackPromotionInput{})

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())

	var out FeedbackPromotionOutput
	require.NoError(t, env.GetWorkflowResult(&out))
	require.Equal(t, 0, out.Evaluated)
	require.Empty(t, out.Promoted)
}

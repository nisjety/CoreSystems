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

func newPromotionEnv() (*testsuite.TestWorkflowEnvironment, *activities.Activities) {
	var suite testsuite.WorkflowTestSuite
	env := suite.NewTestWorkflowEnvironment()
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	a := activities.NewActivities(logger, nil)
	env.RegisterActivity(a.ValidateSkillBundleActivity)
	env.RegisterActivity(a.RunPromotionGateActivity)
	return env, a
}

// TestSkillPromotionWorkflow_GateSuccessFailsExplicitlyAtRegistryUpdate
// replaces the old "happy path" test. SKILL-2 removed UpdateRegistryActivity
// (it called capability-core's now-removed PromoteSkill RPC), so step 3 can
// no longer succeed even when validation and the gate both pass — it now
// fails immediately and explicitly with ErrRegistryUpdateNotSupported instead
// of retrying a call that could never work.
func TestSkillPromotionWorkflow_GateSuccessFailsExplicitlyAtRegistryUpdate(t *testing.T) {
	env, _ := newPromotionEnv()

	env.OnActivity("ValidateSkillBundleActivity", mock.Anything, mock.Anything).
		Return(activities.SkillValidationOutput{Valid: true}, nil).Once()
	env.OnActivity("RunPromotionGateActivity", mock.Anything, mock.Anything).
		Return(activities.PromotionGateOutput{Passed: true, Checks: []string{"skill_exists", "skill_valid", "source_scope_matches", "target_scope_valid", "scope_changes"}}, nil).Once()

	env.ExecuteWorkflow(SkillPromotionWorkflow, SkillPromotionInput{
		SkillID:   "cap.skill.summarize",
		FromScope: "agent",
		ToScope:   "workspace",
	})

	require.True(t, env.IsWorkflowCompleted())
	err := env.GetWorkflowError()
	require.Error(t, err)
	require.Contains(t, err.Error(), "update registry")
	require.Contains(t, err.Error(), ErrRegistryUpdateNotSupported.Error())
}

func TestSkillPromotionWorkflow_GateFailureStopsBeforeRegistryUpdate(t *testing.T) {
	env, _ := newPromotionEnv()

	env.OnActivity("ValidateSkillBundleActivity", mock.Anything, mock.Anything).
		Return(activities.SkillValidationOutput{Valid: true}, nil).Once()
	env.OnActivity("RunPromotionGateActivity", mock.Anything, mock.Anything).
		Return(activities.PromotionGateOutput{Passed: false, Checks: []string{"source_scope_mismatch"}}, nil).Once()

	env.ExecuteWorkflow(SkillPromotionWorkflow, SkillPromotionInput{
		SkillID:   "cap.skill.summarize",
		FromScope: "agent",
		ToScope:   "workspace",
	})

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())

	var output SkillPromotionOutput
	require.NoError(t, env.GetWorkflowResult(&output))
	require.False(t, output.Promoted)
	require.Equal(t, []string{"source_scope_mismatch"}, output.Checks)
	require.Contains(t, output.Reason, "failed")
}

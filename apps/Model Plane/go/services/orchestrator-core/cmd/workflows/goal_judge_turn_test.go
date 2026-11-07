package workflows

import (
	"context"
	"encoding/json"
	"errors"
	"sort"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/activity"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// goalFireInput is a goal-bound fire (EP-7 S6): a scheduled run whose template names a goal.
func goalFireInput(fireKey string) ScheduledRunInput {
	input := agentLaneInput(fireKey)
	input.Goal = "collect the release notes for the quarter"
	input.GoalID = "goal_01JGOALBOUND"
	return input
}

// expectScheduledFire mocks the start and the one agent-lane step of a fire that completes.
func expectScheduledFire(env *testsuite.TestWorkflowEnvironment, input ScheduledRunInput) {
	env.OnActivity(
		"StartScheduledRunActivity",
		mock.Anything, input.RunID, input.ThreadID, input.OrgID, input.SpaceRef, input.SubjectID,
		input.ScheduleID, input.FireKey, input.TemplateDigest, input.IdempotencyKey, input.Goal, "execute",
		input.ContractJSON, input.ContractDigest, input.ContractExpiresAt,
	).Return(activities.RunMetadata{
		RunID: input.RunID, ThreadID: input.ThreadID, OrgID: input.OrgID,
		UserID: activities.SystemActorID, AgentLane: true,
	}, nil).Once()
	env.OnActivity("ExecuteScheduledStepActivity", mock.Anything, mock.Anything).
		Return(activities.StepResult{StepIndex: 0, ToolName: "scheduled-step", Completed: true, RoundsExecuted: 2}, nil).Once()
}

// registerGoalJudge stands in for the judge activity and records what it was given.
func registerGoalJudge(env *testsuite.TestWorkflowEnvironment, result error) *[]GoalJudgeTurnInput {
	calls := &[]GoalJudgeTurnInput{}
	env.RegisterActivityWithOptions(func(_ context.Context, in GoalJudgeTurnInput) error {
		*calls = append(*calls, in)
		return result
	}, activity.RegisterOptions{Name: "JudgeGoalTurnActivity"})
	return calls
}

// TestScheduledRunSupervisionJudgesGoalTurnBehindVersion verifies that a goal-bound fire asks for
// its verdict after the run completes, that a history from before the change (DefaultVersion)
// completes without the activity, and that a fire without a goal never reaches the marker.
func TestScheduledRunSupervisionJudgesGoalTurnBehindVersion(t *testing.T) {
	t.Run("new history judges once after completion", func(t *testing.T) {
		env, _, stub := newTestEnv(t)
		input := goalFireInput("fire-goal-judged")
		expectScheduledFire(env, input)
		calls := registerGoalJudge(env, nil)

		env.ExecuteWorkflow(ScheduledRunSupervision, input)

		require.True(t, env.IsWorkflowCompleted())
		require.NoError(t, env.GetWorkflowError())
		_, decoded := decodeOnlyEnvelope(t, stub.Events())
		assert.Equal(t, "RUN_COMPLETED", decoded.EventType)
		require.Len(t, *calls, 1)
		assert.Equal(t, GoalJudgeTurnInput{
			GoalID: input.GoalID, RunID: input.RunID, OrgID: input.OrgID,
			ScheduleID: input.ScheduleID, FireKey: input.FireKey,
		}, (*calls)[0])
		env.AssertExpectations(t)
	})

	t.Run("pre-change history is not judged", func(t *testing.T) {
		env, _, _ := newTestEnv(t)
		input := goalFireInput("fire-goal-replayed")
		expectScheduledFire(env, input)
		calls := registerGoalJudge(env, nil)
		env.OnGetVersion(goalJudgeTurnChange, workflow.DefaultVersion, goalJudgeTurnVersion).
			Return(workflow.DefaultVersion)

		env.ExecuteWorkflow(ScheduledRunSupervision, input)

		require.True(t, env.IsWorkflowCompleted())
		require.NoError(t, env.GetWorkflowError())
		assert.Empty(t, *calls)
	})

	t.Run("a fire without a goal is not judged", func(t *testing.T) {
		env, _, _ := newTestEnv(t)
		input := agentLaneInput("fire-no-goal")
		expectScheduledFire(env, input)
		calls := registerGoalJudge(env, nil)

		env.ExecuteWorkflow(ScheduledRunSupervision, input)

		require.True(t, env.IsWorkflowCompleted())
		require.NoError(t, env.GetWorkflowError())
		assert.Empty(t, *calls)
	})

	t.Run("a failed judge never fails the completed run", func(t *testing.T) {
		env, _, stub := newTestEnv(t)
		input := goalFireInput("fire-goal-judge-down")
		expectScheduledFire(env, input)
		calls := registerGoalJudge(env, errors.New("judge unavailable"))

		env.ExecuteWorkflow(ScheduledRunSupervision, input)

		require.True(t, env.IsWorkflowCompleted())
		require.NoError(t, env.GetWorkflowError())
		_, decoded := decodeOnlyEnvelope(t, stub.Events())
		assert.Equal(t, "RUN_COMPLETED", decoded.EventType)
		assert.NotEmpty(t, *calls)
	})
}

// TestJudgeGoalTurnActivityCarriesIdsOnly pins the activity input to ids: the objective, the
// criteria and the answer must never enter Temporal history (register D2-318 is the leak not to
// copy).
func TestJudgeGoalTurnActivityCarriesIdsOnly(t *testing.T) {
	input := goalFireInput("fire-goal-ids")
	encoded, err := json.Marshal(GoalJudgeTurnInput{
		GoalID: input.GoalID, RunID: input.RunID, OrgID: input.OrgID,
		ScheduleID: input.ScheduleID, FireKey: input.FireKey,
	})
	require.NoError(t, err)

	var fields map[string]any
	require.NoError(t, json.Unmarshal(encoded, &fields))
	keys := make([]string, 0, len(fields))
	for key := range fields {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	assert.Equal(t, []string{"fire_key", "goal_id", "org_id", "run_id", "schedule_id"}, keys)
	assert.NotContains(t, string(encoded), input.Goal)
	assert.NotContains(t, string(encoded), "release notes")
}

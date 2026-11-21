package workflows

import (
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// TestScheduledStepIntentStatesTheExtSkillsWordOnlyFromTheFireInput pins P2-ec-01-rel1 in the step
// intent: the word the fire's input carries is the word the step carries, and an input without one
// carries none.
func TestScheduledStepIntentStatesTheExtSkillsWordOnlyFromTheFireInput(t *testing.T) {
	stated := scheduledStepIntent(ScheduledRunInput{ExtSkills: "approved_only"}, 0)
	assert.Equal(t, "approved_only", stated.ExtSkills)

	unstated := scheduledStepIntent(ScheduledRunInput{}, 0)
	assert.Empty(t, unstated.ExtSkills, "a fire with no stated word is not enforced")

	// The step intent is persisted as a Temporal activity input. With no word stated the key is absent, so a
	// switch-off step's input is what it was; with a word stated it is carried as a JSON string.
	unstatedJSON, err := json.Marshal(unstated)
	require.NoError(t, err)
	assert.NotContains(t, string(unstatedJSON), "ExtSkills", "a switch-off step's input keeps no ext.skills key")
	statedJSON, err := json.Marshal(stated)
	require.NoError(t, err)
	assert.Contains(t, string(statedJSON), `"ExtSkills":"approved_only"`)
}

// TestScheduledRun_StepActivityReceivesTheExtSkillsWord runs a scheduled fire that carries an
// ext.skills word end to end through the workflow, and checks the step activity receives it.
func TestScheduledRun_StepActivityReceivesTheExtSkillsWord(t *testing.T) {
	env, _, _ := newTestEnv(t)
	input := ScheduledRunInput{
		RunID: "task-schedule-1", ThreadID: "thread-schedule-1", OrgID: "org-1",
		SpaceRef: "space-1", SubjectID: "user-1",
		ScheduleID: "schedule-1", FireKey: "2026-08-14T00:00:00Z",
		TemplateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		PolicyDigest:   "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		IdempotencyKey: "schedule-1:2026-08-14T00:00:00Z",
		Goal:           "summarise the work queue", Policy: "execute", Retention: activities.RetentionDurable,
		ExtSkills: "approved_only",
	}
	env.OnActivity(
		"StartScheduledRunActivity",
		mock.Anything, input.RunID, input.ThreadID, input.OrgID, input.SpaceRef, input.SubjectID,
		input.ScheduleID, input.FireKey, input.TemplateDigest, input.IdempotencyKey, input.Goal, "execute",
		input.ContractJSON, input.ContractDigest, input.ContractExpiresAt,
	).Return(activities.RunMetadata{
		RunID: input.RunID, ThreadID: input.ThreadID, OrgID: input.OrgID,
		UserID: activities.SystemActorID,
	}, nil).Once()
	env.OnActivity("ExecuteScheduledStepActivity", mock.Anything, mock.MatchedBy(func(intent activities.ScheduledStepExecutionIntent) bool {
		return intent.RunID == input.RunID && intent.ExtSkills == "approved_only"
	})).Return(activities.StepResult{StepIndex: 0, Completed: true, ToolName: "scheduled-step"}, nil).Once()

	env.ExecuteWorkflow(ScheduledRunSupervision, input)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	env.AssertExpectations(t)
}

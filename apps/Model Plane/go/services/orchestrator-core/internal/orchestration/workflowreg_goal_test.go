package orchestration

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/workflows"
)

// withGoalJudgeActivity sets the EP-7 S6 switch for one test.
func withGoalJudgeActivity(t *testing.T, on bool) {
	t.Helper()
	previous := goalJudgeActivityEnabled
	goalJudgeActivityEnabled = func() bool { return on }
	t.Cleanup(func() { goalJudgeActivityEnabled = previous })
}

func goalBoundTemplate(goalID any) map[string]any {
	return map[string]any{
		"title":         "Keep working: release notes",
		"description":   "collect the release notes for the quarter",
		"purpose":       "goal",
		"goal_id":       goalID,
		"goal_max_runs": 3,
	}
}

// TestScheduledRunInputCarriesGoalIDOnlyWhileTheJudgeSwitchIsOn verifies that the goal id of a
// goal-bound template reaches the workflow input only with ORCHESTRATOR_GOAL_JUDGE_ACTIVITY_ENABLED
// on, so with the switch off the input is what it was before EP-7 S6.
func TestScheduledRunInputCarriesGoalIDOnlyWhileTheJudgeSwitchIsOn(t *testing.T) {
	tenancy := Tenancy{OrgID: "org-a", RunID: "task_scheduled_1", RetentionAttested: true}
	templateJSON, templateDigest := scheduledTemplateInput(t, goalBoundTemplate("goal_01JGOALBOUND"))

	withGoalJudgeActivity(t, false)
	built, err := buildScheduledRunInput(scheduledRunBuildRequest(t, templateJSON, templateDigest), tenancy)
	require.NoError(t, err)
	assert.Empty(t, built.(workflows.ScheduledRunInput).GoalID)

	withGoalJudgeActivity(t, true)
	built, err = buildScheduledRunInput(scheduledRunBuildRequest(t, templateJSON, templateDigest), tenancy)
	require.NoError(t, err)
	assert.Equal(t, "goal_01JGOALBOUND", built.(workflows.ScheduledRunInput).GoalID)

	plainJSON, plainDigest := scheduledTemplateInput(t, map[string]any{"title": "Nightly queue review"})
	built, err = buildScheduledRunInput(scheduledRunBuildRequest(t, plainJSON, plainDigest), tenancy)
	require.NoError(t, err)
	assert.Empty(t, built.(workflows.ScheduledRunInput).GoalID)
}

// TestScheduledRunInputRefusesAnInvalidGoalID verifies that a template naming something that is not
// a goal id is refused rather than carried into Temporal.
func TestScheduledRunInputRefusesAnInvalidGoalID(t *testing.T) {
	tenancy := Tenancy{OrgID: "org-a", RunID: "task_scheduled_1", RetentionAttested: true}
	withGoalJudgeActivity(t, true)
	for _, goalID := range []any{"thread-1", "goal_", "goal_has space", 7, "goal_" + string(make([]byte, 80))} {
		templateJSON, templateDigest := scheduledTemplateInput(t, goalBoundTemplate(goalID))
		_, err := buildScheduledRunInput(scheduledRunBuildRequest(t, templateJSON, templateDigest), tenancy)
		assert.Error(t, err, "goal id %v", goalID)
	}
}

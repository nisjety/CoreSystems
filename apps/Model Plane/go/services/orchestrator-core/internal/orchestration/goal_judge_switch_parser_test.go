package orchestration

import "testing"

// Plan 3.4 for ORCHESTRATOR_GOAL_JUDGE_ACTIVITY_ENABLED (EP-7 S6): the production reader turns
// the judge on for the literal "true" only. The other tests in this package replace the reader;
// this one calls the one the binary runs.
func TestGoalJudgeActivitySwitchIsOnlyTheLiteralTrue(t *testing.T) {
	const name = "ORCHESTRATOR_GOAL_JUDGE_ACTIVITY_ENABLED"
	for _, value := range []string{"", "false", "TRUE", "True", " true", "true ", "1", "yes", "on"} {
		t.Setenv(name, value)
		if goalJudgeActivityEnabled() {
			t.Errorf("%s=%q turned the goal judge activity on", name, value)
		}
	}
	t.Setenv(name, "true")
	if !goalJudgeActivityEnabled() {
		t.Fatalf("%s=true did not turn the goal judge activity on", name)
	}
}

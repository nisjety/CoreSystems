package cron

import (
	"context"
	"encoding/json"
	"net/http"
	"testing"
	"time"
)

// goalTemplate is the live template of runNowNow with extra fields merged in, so a goal-bound
// schedule differs from a plain one only in the goal fields.
func goalTemplate(t *testing.T, extra map[string]any) json.RawMessage {
	t.Helper()
	var fields map[string]any
	if err := json.Unmarshal(guardTemplate(t, runNowNow().Add(30*24*time.Hour)), &fields); err != nil {
		t.Fatal(err)
	}
	for key, value := range extra {
		fields[key] = value
	}
	raw, err := json.Marshal(fields)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

// W3-EP-7-S6-d: a goal-bound schedule runs only on its own cadence (one driver per goal), so a
// manual fire is refused with 409 goal_schedule_run_now_refused before Control is asked and
// before anything is written. The shapes are the ones the goal producers write (model-gateway
// goal_unattended.rs, session-core grpc.rs): purpose "goal" with a goal_id.
func TestRunNowRefusesGoalBoundSchedule(t *testing.T) {
	cases := map[string]map[string]any{
		"purpose goal with goal id": {"purpose": "goal", "goal_id": "goal_01JABCDEF", "goal_max_runs": 3},
		"purpose goal alone":        {"purpose": "goal"},
		"goal id alone":             {"goal_id": "goal_01JABCDEF"},
		"goal id that is not text":  {"goal_id": 7},
	}
	for name, extra := range cases {
		t.Run(name, func(t *testing.T) {
			tx := liveRunNowTx(t)
			tx.template = goalTemplate(t, extra)
			authorizer := &recordingRunNowAuthorizer{}
			result, refusal, err := RunNow(context.Background(), tx, authorizer, runNowAs("user-1"), runNowNow())
			wantRefusal(t, refusal, err, http.StatusConflict, "goal_schedule_run_now_refused")
			if result != (RunNowResult{}) {
				t.Fatalf("a refused press returned a fire: %+v", result)
			}
			if authorizer.calls != 0 || len(tx.execs) != 0 || tx.committed {
				t.Fatalf("a goal-bound press reached Control or wrote: calls=%d writes=%+v committed=%t",
					authorizer.calls, tx.execs, tx.committed)
			}
			if refusal.Message != "a goal-bound schedule runs only on its own cadence" {
				t.Fatalf("refusal message = %q", refusal.Message)
			}
		})
	}
}

// The creator check still comes first: someone else pressing Run now on a goal schedule learns
// only that it is not theirs.
func TestRunNowGoalBoundScheduleStillRefusesNonCreatorFirst(t *testing.T) {
	tx := liveRunNowTx(t)
	tx.template = goalTemplate(t, map[string]any{"purpose": "goal", "goal_id": "goal_01JABCDEF"})
	_, refusal, err := RunNow(context.Background(), tx, &recordingRunNowAuthorizer{}, runNowAs("user-2"), runNowNow())
	wantRefusal(t, refusal, err, http.StatusForbidden, "run_now_not_creator")
}

// A retried request id on a goal schedule finds no earlier manual fire to return: the refusal
// comes before the dedup lookup, so even an old manual fire key cannot reopen the button.
func TestRunNowGoalBoundRefusalComesBeforeDedup(t *testing.T) {
	tx := liveRunNowTx(t)
	tx.template = goalTemplate(t, map[string]any{"purpose": "goal", "goal_id": "goal_01JABCDEF"})
	tx.existing = &RunNowResult{FireID: "cronfire_old", TaskID: "task_old", Status: "pending"}
	_, refusal, err := RunNow(context.Background(), tx, &recordingRunNowAuthorizer{}, runNowAs("user-1"), runNowNow())
	wantRefusal(t, refusal, err, http.StatusConflict, "goal_schedule_run_now_refused")
}

// Plain schedules are untouched: no purpose, another purpose, an empty or null goal id.
func TestRunNowStillFiresSchedulesThatNameNoGoal(t *testing.T) {
	cases := map[string]map[string]any{
		"no goal fields":           {},
		"other purpose":            {"purpose": "digest"},
		"empty goal id":            {"goal_id": ""},
		"null goal id":             {"goal_id": nil},
		"purpose GOAL is not goal": {"purpose": "Goal"},
	}
	for name, extra := range cases {
		t.Run(name, func(t *testing.T) {
			tx := liveRunNowTx(t)
			tx.template = goalTemplate(t, extra)
			authorizer := &recordingRunNowAuthorizer{}
			result, refusal, err := RunNow(context.Background(), tx, authorizer, runNowAs("user-1"), runNowNow())
			if err != nil || refusal != nil {
				t.Fatalf("refusal=%+v err=%v", refusal, err)
			}
			if result.Status != "pending" || authorizer.calls != 1 || !tx.committed {
				t.Fatalf("result=%+v calls=%d committed=%t", result, authorizer.calls, tx.committed)
			}
		})
	}
}

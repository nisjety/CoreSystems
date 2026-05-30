package temporalreg

import (
	"reflect"
	"sort"
	"testing"
)

func TestTaskQueuesCanonical(t *testing.T) {
	want := []string{"mp-session", "mp-inference", "mp-execution"}
	if !reflect.DeepEqual(TaskQueues, want) {
		t.Fatalf("TaskQueues=%v want %v", TaskQueues, want)
	}
}

func TestWorkflowsPerQueue(t *testing.T) {
	cases := map[string][]string{
		"mp-session":   {"SessionWorkflow", "RunWorkflow"},
		"mp-inference": {"InferenceWorkflow"},
		"mp-execution": {"ExecutionWorkflow"},
	}
	for q, want := range cases {
		got, ok := Workflows[q]
		if !ok {
			t.Fatalf("Workflows missing queue %q", q)
		}
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("Workflows[%q]=%v want %v", q, got, want)
		}
	}
	if len(Workflows) != len(cases) {
		t.Fatalf("Workflows has %d entries want %d", len(Workflows), len(cases))
	}
}

func TestActivitiesPerQueue(t *testing.T) {
	cases := map[string][]string{
		"mp-session":   {"PersistRunStart", "PersistRunEnd", "EmitEvent", "CreateCheckpoint"},
		"mp-inference": {"InvokeModel", "RecordUsage", "EmitEvent"},
		"mp-execution": {"ExecuteStep", "ToolCallActivity", "EmitEvent"},
	}
	for q, want := range cases {
		got, ok := Activities[q]
		if !ok {
			t.Fatalf("Activities missing queue %q", q)
		}
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("Activities[%q]=%v want %v", q, got, want)
		}
	}
}

func TestAllActivitiesDedupedSorted(t *testing.T) {
	got := AllActivities()
	want := []string{
		"CreateCheckpoint",
		"EmitEvent",
		"ExecuteStep",
		"InvokeModel",
		"PersistRunEnd",
		"PersistRunStart",
		"RecordUsage",
		"ToolCallActivity",
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("AllActivities()=%v want %v", got, want)
	}
	if !sort.StringsAreSorted(got) {
		t.Fatalf("AllActivities() not sorted: %v", got)
	}
}

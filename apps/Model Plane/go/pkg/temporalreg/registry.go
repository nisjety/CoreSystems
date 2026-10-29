// Package temporalreg is the canonical registry of Temporal task queues,
// workflow names, and activity names for the Model Plane. It is the Go
// counterpart to the Rust mp_events::temporal module; both must stay in sync.
package temporalreg

import "sort"

// TaskQueues lists the canonical Temporal task queue names in declaration order.
var TaskQueues = []string{"mp-session", "mp-inference", "mp-execution"}

// Workflows maps each task queue to its canonical workflow type names.
var Workflows = map[string][]string{
	"mp-session":   {"SessionWorkflow", "RunWorkflow"},
	"mp-inference": {"InferenceWorkflow"},
	"mp-execution": {"ExecutionWorkflow"},
}

// Activities maps each task queue to its canonical activity type names.
var Activities = map[string][]string{
	"mp-session":   {"PersistRunStart", "PersistRunEnd", "EmitEvent", "CreateCheckpoint"},
	"mp-inference": {"InvokeModel", "RecordUsage", "EmitEvent"},
	"mp-execution": {"ExecuteStep", "ToolCallActivity", "EmitEvent"},
}

// AllActivities returns the deduplicated, lexicographically sorted union of
// every activity name registered across every task queue.
func AllActivities() []string {
	seen := make(map[string]struct{})
	for _, names := range Activities {
		for _, n := range names {
			seen[n] = struct{}{}
		}
	}
	out := make([]string, 0, len(seen))
	for n := range seen {
		out = append(out, n)
	}
	sort.Strings(out)
	return out
}

package main

import "testing"

// TestAcceptsPublishOnlyDispatch covers AUTO-3's opt-in acknowledgment gate:
// only an exact "true" (case-insensitive) counts as a deployment explicitly
// declaring it runs its own consumer of mp.v1.capability.task.dispatched.
// Every other value — unset, empty, or anything else — must default to
// false, so NatsDispatcher's publish-then-fail safety net stays armed unless
// a deployment opts in on purpose.
func TestAcceptsPublishOnlyDispatch(t *testing.T) {
	tests := []struct {
		name  string
		value string
		set   bool
		want  bool
	}{
		{name: "unset defaults to false", set: false, want: false},
		{name: "empty string is false", value: "", set: true, want: false},
		{name: "true enables it", value: "true", set: true, want: true},
		{name: "case-insensitive TRUE enables it", value: "TRUE", set: true, want: true},
		{name: "whitespace-padded true enables it", value: "  true  ", set: true, want: true},
		{name: "false stays false", value: "false", set: true, want: false},
		{name: "garbage value stays false", value: "yes-please", set: true, want: false},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if test.set {
				t.Setenv("TASK_EXECUTOR_ACCEPT_PUBLISH_ONLY_DISPATCH", test.value)
			}
			if got := acceptsPublishOnlyDispatch(); got != test.want {
				t.Fatalf("acceptsPublishOnlyDispatch() = %v, want %v", got, test.want)
			}
		})
	}
}

// TestTaskExecutorEnabled covers the executor on/off decision, including the
// AUTO-3 addition: an explicit "true" without a workflow dispatcher is still
// honoured (unchanged behavior — a deployment may force it), but now depends
// on TASK_EXECUTOR_ACCEPT_PUBLISH_ONLY_DISPATCH only for which log level
// fires, not for the on/off decision itself, which was already true-always.
func TestTaskExecutorEnabled(t *testing.T) {
	tests := []struct {
		name           string
		envValue       string
		envSet         bool
		workflowBacked bool
		want           bool
	}{
		{name: "unset + workflow backed -> on", envSet: false, workflowBacked: true, want: true},
		{name: "unset + not workflow backed -> off (safe default)", envSet: false, workflowBacked: false, want: false},
		{name: "false always wins even when workflow backed", envValue: "false", envSet: true, workflowBacked: true, want: false},
		{name: "true forces on even without workflow dispatcher", envValue: "true", envSet: true, workflowBacked: false, want: true},
		{name: "true stays on when workflow backed too", envValue: "true", envSet: true, workflowBacked: true, want: true},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if test.envSet {
				t.Setenv("TASK_EXECUTOR_ENABLED", test.envValue)
			}
			if got := taskExecutorEnabled(test.workflowBacked); got != test.want {
				t.Fatalf("taskExecutorEnabled(%v) = %v, want %v", test.workflowBacked, got, test.want)
			}
		})
	}
}

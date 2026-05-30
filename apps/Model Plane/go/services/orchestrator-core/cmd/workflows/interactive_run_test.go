package workflows

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/activity"
	"go.temporal.io/sdk/testsuite"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/natsx"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// capturingRawPublisher records every (subject, data) pair published so tests
// can assert on the emitted NATS envelopes.
type capturingRawPublisher struct {
	mu     sync.Mutex
	events []capturedEvent
}

type capturedEvent struct {
	subject string
	data    []byte
}

func (c *capturingRawPublisher) Publish(subject string, data []byte) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	// copy data since the publisher may reuse the buffer
	buf := make([]byte, len(data))
	copy(buf, data)
	c.events = append(c.events, capturedEvent{subject: subject, data: buf})
	return nil
}

func (c *capturingRawPublisher) Events() []capturedEvent {
	c.mu.Lock()
	defer c.mu.Unlock()
	out := make([]capturedEvent, len(c.events))
	copy(out, c.events)
	return out
}

// newTestEnv builds a TestWorkflowEnvironment with an Activities struct that
// has a capturing publisher wired in (nil gRPC clients → fallback paths).
func newTestEnv(t *testing.T) (*testsuite.TestWorkflowEnvironment, *activities.Activities, *capturingRawPublisher) {
	t.Helper()
	var suite testsuite.WorkflowTestSuite
	env := suite.NewTestWorkflowEnvironment()

	stub := &capturingRawPublisher{}
	pub := natsx.NewPublisher(stub, natsx.ModeV1Only)

	// Discard logger keeps test output clean.
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	a := activities.NewActivities(logger, nil)
	a.SetPublisher(pub)

	// Register only the activities invoked by the interactive workflow,
	// using explicit names so string-based lookups in interactive_run.go
	// resolve correctly. Registering the whole struct would trip over
	// non-activity helper methods (e.g. SetPublisher) that do not match
	// Temporal's activity signature contract.
	env.RegisterActivityWithOptions(a.StartRunActivity, activity.RegisterOptions{Name: "StartRunActivity"})
	env.RegisterActivityWithOptions(a.ExecuteStepLoopActivity, activity.RegisterOptions{Name: "ExecuteStepLoopActivity"})
	env.RegisterActivityWithOptions(a.CompleteRunActivity, activity.RegisterOptions{Name: "CompleteRunActivity"})
	env.RegisterActivityWithOptions(a.FailRunActivity, activity.RegisterOptions{Name: "FailRunActivity"})

	return env, a, stub
}

func decodeOnlyEnvelope(t *testing.T, events []capturedEvent) (capturedEvent, envelope.Envelope) {
	t.Helper()
	require.Len(t, events, 1, "expected exactly one published envelope")
	var env envelope.Envelope
	require.NoError(t, json.Unmarshal(events[0].data, &env))
	return events[0], env
}

// TestInteractiveRun_HappyPath_EmitsRunCompletedWithOrgID drives the workflow
// through start → step loop → complete with nil clients (fallback paths) and
// verifies the RUN_COMPLETED envelope carries the input OrgID.
func TestInteractiveRun_HappyPath_EmitsRunCompletedWithOrgID(t *testing.T) {
	env, a, stub := newTestEnv(t)
	_ = a

	input := InteractiveRunInput{
		RunID:    "run-happy-1",
		ThreadID: "thread-1",
		Goal:     "test goal",
		Policy:   "default",
		OrgID:    "org-happy",
		UserID:   "user-1",
	}

	env.ExecuteWorkflow(InteractiveRunSupervision, input)

	require.True(t, env.IsWorkflowCompleted(), "workflow should complete")
	require.NoError(t, env.GetWorkflowError(), "workflow should not error")

	evt, decoded := decodeOnlyEnvelope(t, stub.Events())
	assert.Equal(t, natsx.RunEventSubject(input.RunID), evt.subject)
	assert.Equal(t, "RUN_COMPLETED", decoded.EventType)
	assert.Equal(t, input.OrgID, decoded.OrgID, "OrgID must propagate into envelope")
	assert.Equal(t, "run/"+input.RunID, decoded.ResourceRef)
	assert.Equal(t, input.RunID+":completed", decoded.IdempotencyKey)
	assert.Equal(t, "orchestrator-core", decoded.Producer)
	assert.Equal(t, uint32(1), decoded.SchemaVersion)

	var payload map[string]any
	require.NoError(t, json.Unmarshal(decoded.Payload, &payload))
	assert.Equal(t, input.RunID, payload["run_id"])
	_, hasSummary := payload["summary"]
	assert.True(t, hasSummary, "completion payload should include summary")
}

// TestInteractiveRun_StartRunFailure_EmitsRunFailedWithOrgID forces
// StartRunActivity to return an error and verifies the workflow emits a
// RUN_FAILED envelope carrying the input OrgID and the wrapped reason.
func TestInteractiveRun_StartRunFailure_EmitsRunFailedWithOrgID(t *testing.T) {
	env, _, stub := newTestEnv(t)

	// Override StartRunActivity to return an error on every attempt.
	env.OnActivity("StartRunActivity", mock.Anything, mock.Anything, mock.Anything, mock.Anything, mock.Anything).
		Return(activities.RunMetadata{}, errors.New("downstream boom"))

	input := InteractiveRunInput{
		RunID:    "run-fail-1",
		ThreadID: "thread-1",
		Goal:     "test goal",
		Policy:   "default",
		OrgID:    "org-fail",
		UserID:   "user-1",
	}

	env.ExecuteWorkflow(InteractiveRunSupervision, input)

	require.True(t, env.IsWorkflowCompleted())
	wfErr := env.GetWorkflowError()
	require.Error(t, wfErr, "workflow should surface a terminal error")
	assert.Contains(t, wfErr.Error(), "start run")

	evt, decoded := decodeOnlyEnvelope(t, stub.Events())
	assert.Equal(t, natsx.RunEventSubject(input.RunID), evt.subject)
	assert.Equal(t, "RUN_FAILED", decoded.EventType)
	assert.Equal(t, input.OrgID, decoded.OrgID, "OrgID must propagate into failure envelope")
	assert.Equal(t, "run/"+input.RunID, decoded.ResourceRef)
	assert.Equal(t, input.RunID+":failed", decoded.IdempotencyKey)

	var payload map[string]any
	require.NoError(t, json.Unmarshal(decoded.Payload, &payload))
	assert.Equal(t, input.RunID, payload["run_id"])
	reason, _ := payload["reason"].(string)
	assert.True(t, strings.Contains(reason, "start run"), "reason should describe start-run failure, got %q", reason)
}

// TestInteractiveRun_CancelSignal_EmitsRunFailed sends a cancel signal before
// the workflow gets past the StartRun checkpoint and verifies a RUN_FAILED
// envelope is emitted with a cancellation reason.
func TestInteractiveRun_CancelSignal_EmitsRunFailed(t *testing.T) {
	env, _, stub := newTestEnv(t)

	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(SignalCancel, nil)
	}, 0)

	input := InteractiveRunInput{
		RunID:    "run-cancel-1",
		ThreadID: "thread-1",
		Goal:     "test goal",
		Policy:   "default",
		OrgID:    "org-cancel",
		UserID:   "user-1",
	}

	env.ExecuteWorkflow(InteractiveRunSupervision, input)

	require.True(t, env.IsWorkflowCompleted())
	wfErr := env.GetWorkflowError()
	require.Error(t, wfErr, "workflow should surface a terminal error after cancel")
	assert.Contains(t, wfErr.Error(), "cancelled")

	evt, decoded := decodeOnlyEnvelope(t, stub.Events())
	assert.Equal(t, natsx.RunEventSubject(input.RunID), evt.subject)
	assert.Equal(t, "RUN_FAILED", decoded.EventType)
	assert.Equal(t, input.OrgID, decoded.OrgID)
	assert.Equal(t, input.RunID+":failed", decoded.IdempotencyKey)

	var payload map[string]any
	require.NoError(t, json.Unmarshal(decoded.Payload, &payload))
	reason, _ := payload["reason"].(string)
	assert.True(t, strings.Contains(reason, "cancelled"), "reason should describe cancellation, got %q", reason)
}

// TestInteractiveRun_ApprovalSignal_AllowsCompletion stages a step loop that
// requires approval, signals approval, then expects a second step loop run
// returning Completed=true → RUN_COMPLETED envelope.
func TestInteractiveRun_ApprovalSignal_AllowsCompletion(t *testing.T) {
	env, _, stub := newTestEnv(t)

	first := activities.StepLoopOutput{
		Steps: []activities.StepResult{
			{StepIndex: 0, ToolName: "needs-human", NeedsApproval: true},
		},
		Completed: false,
	}
	second := activities.StepLoopOutput{
		Steps: []activities.StepResult{
			{StepIndex: 0, ToolName: "final", Completed: true},
		},
		Completed: true,
		Summary:   "done",
	}
	call := env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.Anything).Return(first, nil).Once()
	env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.Anything).Return(second, nil).NotBefore(call)

	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(SignalApproval, nil)
	}, time.Millisecond)

	input := InteractiveRunInput{
		RunID:    "run-approve-1",
		ThreadID: "thread-1",
		Goal:     "test goal",
		Policy:   "default",
		OrgID:    "org-approve",
		UserID:   "user-1",
	}

	env.ExecuteWorkflow(InteractiveRunSupervision, input)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())

	evt, decoded := decodeOnlyEnvelope(t, stub.Events())
	assert.Equal(t, natsx.RunEventSubject(input.RunID), evt.subject)
	assert.Equal(t, "RUN_COMPLETED", decoded.EventType)
	assert.Equal(t, input.OrgID, decoded.OrgID)
	assert.Equal(t, input.RunID+":completed", decoded.IdempotencyKey)
}

// TestInteractiveRun_StepLoopFailure_EmitsRunFailed forces the step loop to
// error and verifies the workflow surfaces the failure via FailRunActivity.
func TestInteractiveRun_StepLoopFailure_EmitsRunFailed(t *testing.T) {
	env, _, stub := newTestEnv(t)

	env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.Anything).
		Return(activities.StepLoopOutput{}, errors.New("step boom"))

	input := InteractiveRunInput{
		RunID:    "run-steploop-1",
		ThreadID: "thread-1",
		Goal:     "test goal",
		Policy:   "default",
		OrgID:    "org-steploop",
		UserID:   "user-1",
	}

	env.ExecuteWorkflow(InteractiveRunSupervision, input)

	require.True(t, env.IsWorkflowCompleted())
	wfErr := env.GetWorkflowError()
	require.Error(t, wfErr)
	assert.Contains(t, wfErr.Error(), "step loop")

	_, decoded := decodeOnlyEnvelope(t, stub.Events())
	assert.Equal(t, "RUN_FAILED", decoded.EventType)
	assert.Equal(t, input.OrgID, decoded.OrgID)

	var payload map[string]any
	require.NoError(t, json.Unmarshal(decoded.Payload, &payload))
	reason, _ := payload["reason"].(string)
	assert.True(t, strings.Contains(reason, "step loop"), "reason should describe step-loop failure, got %q", reason)
}

// TestInteractiveRun_CompleteRunFailure_FallsBackToFailRun forces
// CompleteRunActivity to error after a successful step loop and verifies the
// workflow falls back to FailRunActivity which emits RUN_FAILED.
func TestInteractiveRun_CompleteRunFailure_FallsBackToFailRun(t *testing.T) {
	env, _, stub := newTestEnv(t)

	env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.Anything).
		Return(activities.StepLoopOutput{
			Steps:     []activities.StepResult{{StepIndex: 0, ToolName: "final", Completed: true}},
			Completed: true,
			Summary:   "ok",
		}, nil)

	env.OnActivity("CompleteRunActivity", mock.Anything, mock.Anything).
		Return(errors.New("complete boom"))

	input := InteractiveRunInput{
		RunID:    "run-complete-fail-1",
		ThreadID: "thread-1",
		Goal:     "test goal",
		Policy:   "default",
		OrgID:    "org-complete-fail",
		UserID:   "user-1",
	}

	env.ExecuteWorkflow(InteractiveRunSupervision, input)

	require.True(t, env.IsWorkflowCompleted())
	wfErr := env.GetWorkflowError()
	require.Error(t, wfErr)
	assert.Contains(t, wfErr.Error(), "complete run")

	_, decoded := decodeOnlyEnvelope(t, stub.Events())
	assert.Equal(t, "RUN_FAILED", decoded.EventType)
	assert.Equal(t, input.OrgID, decoded.OrgID)
	assert.Equal(t, input.RunID+":failed", decoded.IdempotencyKey)

	var payload map[string]any
	require.NoError(t, json.Unmarshal(decoded.Payload, &payload))
	reason, _ := payload["reason"].(string)
	assert.True(t, strings.Contains(reason, "complete run"), "reason should describe complete-run failure, got %q", reason)
}

func TestInteractiveRun_CancelMidActivity_StillEmitsRunFailed(t *testing.T) {
	env, _, stub := newTestEnv(t)
	env.OnActivity("ExecuteStepLoopActivity", mock.Anything, mock.Anything).
		Return(activities.StepLoopOutput{}, context.Canceled)
	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(SignalCancel, nil)
	}, 100*time.Millisecond)
	input := InteractiveRunInput{
		RunID:    "run-cancel-mid-1",
		ThreadID: "thread-1",
		Goal:     "test goal",
		Policy:   "default",
		OrgID:    "org-cancel-mid",
		UserID:   "user-1",
	}
	env.ExecuteWorkflow(InteractiveRunSupervision, input)
	require.True(t, env.IsWorkflowCompleted())
	require.Error(t, env.GetWorkflowError())
	evt, decoded := decodeOnlyEnvelope(t, stub.Events())
	assert.Equal(t, natsx.RunEventSubject(input.RunID), evt.subject)
	assert.Equal(t, "RUN_FAILED", decoded.EventType)
	assert.Equal(t, input.OrgID, decoded.OrgID)
	assert.Equal(t, "user-1", decoded.UserID)
	assert.Equal(t, input.RunID, decoded.CorrelationID)
	assert.Equal(t, input.RunID+":failed", decoded.IdempotencyKey)
}

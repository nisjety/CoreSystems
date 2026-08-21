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
	env.RegisterActivityWithOptions(a.StartScheduledRunActivity, activity.RegisterOptions{Name: "StartScheduledRunActivity"})
	env.RegisterActivityWithOptions(a.ExecuteScheduledStepActivity, activity.RegisterOptions{Name: "ExecuteScheduledStepActivity"})
	// New runs drive the step loop per turn via ExecuteStepActivity; the legacy
	// whole-loop ExecuteStepLoopActivity stays registered for old-history replay.
	env.RegisterActivityWithOptions(a.ExecuteStepLoopActivity, activity.RegisterOptions{Name: "ExecuteStepLoopActivity"})
	env.RegisterActivityWithOptions(a.ExecuteStepActivity, activity.RegisterOptions{Name: "ExecuteStepActivity"})
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
		// An ordinary run whose issuer attested that its content is retainable.
		// Without an explicit posture the lifecycle payload omits run-derived
		// content by design — see the retention tests at the end of this file.
		Retention: activities.RetentionDurable,
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

// TestScheduledRun_UsesPreparedThreadAndCompletes verifies the dedicated
// schedule lane. It must call StartScheduledRunActivity with the exact
// non-secret preparation facts; it must not route through StartRunActivity,
// which could create a different service thread for the same fire.
func TestScheduledRun_UsesPreparedThreadAndCompletes(t *testing.T) {
	env, _, stub := newTestEnv(t)
	input := ScheduledRunInput{
		RunID: "task-schedule-1", ThreadID: "thread-schedule-1", OrgID: "org-1",
		SpaceRef: "space-1", SubjectID: "user-1",
		ScheduleID: "schedule-1", FireKey: "2026-08-14T00:00:00Z",
		TemplateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		PolicyDigest:   "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		IdempotencyKey: "schedule-1:2026-08-14T00:00:00Z",
		Goal:           "summarise the work queue", Policy: "execute", Retention: activities.RetentionDurable,
	}
	env.OnActivity(
		"StartScheduledRunActivity",
		mock.Anything, input.RunID, input.ThreadID, input.OrgID, input.SpaceRef, input.SubjectID,
		input.ScheduleID, input.FireKey, input.TemplateDigest, input.IdempotencyKey, input.Goal, "execute",
	).Return(activities.RunMetadata{
		RunID: input.RunID, ThreadID: input.ThreadID, OrgID: input.OrgID,
		UserID: activities.SystemActorID,
	}, nil).Once()
	env.OnActivity("ExecuteScheduledStepActivity", mock.Anything, mock.MatchedBy(func(intent activities.ScheduledStepExecutionIntent) bool {
		return intent.RunID == input.RunID && intent.ThreadID == input.ThreadID &&
			intent.OrgID == input.OrgID && intent.SpaceRef == input.SpaceRef &&
			intent.SubjectID == input.SubjectID && intent.ScheduleID == input.ScheduleID &&
			intent.FireKey == input.FireKey && intent.TemplateDigest == input.TemplateDigest &&
			intent.StepID == input.RunID+":step:0" && intent.StepIndex == 0 &&
			intent.IdempotencyKey == input.IdempotencyKey+":step:0" && intent.PolicyDigest != ""
	})).Return(activities.StepResult{StepIndex: 0, Completed: true, ToolName: "scheduled-step"}, nil).Once()

	env.ExecuteWorkflow(ScheduledRunSupervision, input)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	_, decoded := decodeOnlyEnvelope(t, stub.Events())
	assert.Equal(t, "RUN_COMPLETED", decoded.EventType)
	assert.Equal(t, input.OrgID, decoded.OrgID)
	env.AssertExpectations(t)
}

func TestScheduledRun_UnknownOutcomeStopsWithoutAnotherStep(t *testing.T) {
	env, _, stub := newTestEnv(t)
	input := ScheduledRunInput{
		RunID: "task-schedule-unknown", ThreadID: "thread-schedule-unknown", OrgID: "org-1",
		SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-1", FireKey: "fire-unknown",
		TemplateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		PolicyDigest:   "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		IdempotencyKey: "schedule-1:fire-unknown", Goal: "summarise the work queue", Policy: "execute",
		Retention: activities.RetentionDurable,
	}
	env.OnActivity(
		"StartScheduledRunActivity",
		mock.Anything, input.RunID, input.ThreadID, input.OrgID, input.SpaceRef, input.SubjectID,
		input.ScheduleID, input.FireKey, input.TemplateDigest, input.IdempotencyKey, input.Goal, "execute",
	).Return(activities.RunMetadata{RunID: input.RunID, ThreadID: input.ThreadID, OrgID: input.OrgID, UserID: activities.SystemActorID}, nil).Once()
	env.OnActivity("ExecuteScheduledStepActivity", mock.Anything, mock.Anything).
		Return(activities.StepResult{
			StepIndex: 0, ToolName: "scheduled-step", UnknownOutcome: true,
			Metadata: map[string]string{"receipt_id": "receipt-unknown"},
		}, nil).Once()

	env.ExecuteWorkflow(ScheduledRunSupervision, input)

	require.True(t, env.IsWorkflowCompleted())
	require.Error(t, env.GetWorkflowError())
	_, decoded := decodeOnlyEnvelope(t, stub.Events())
	assert.Equal(t, "RUN_FAILED", decoded.EventType)
	var payload map[string]any
	require.NoError(t, json.Unmarshal(decoded.Payload, &payload))
	reason, _ := payload["reason"].(string)
	assert.Contains(t, reason, "unknown")
}

// TestScheduledRun_RetryKeepsTheSameStepIdempotencyTuple verifies the Temporal
// retry boundary for a transient worker/transport failure. A retry is allowed
// for an indeterminate activity error, but it must replay the exact same
// deterministic step ID and idempotency key; the owner/Session receipt ledger
// is what makes that repeated delivery safe. This test intentionally does not
// use the non-retryable unknown_outcome result.
func TestScheduledRun_RetryKeepsTheSameStepIdempotencyTuple(t *testing.T) {
	env, _, stub := newTestEnv(t)
	input := ScheduledRunInput{
		RunID: "task-schedule-retry", ThreadID: "thread-schedule-retry", OrgID: "org-1",
		SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-1", FireKey: "fire-retry",
		TemplateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		PolicyDigest:   "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		IdempotencyKey: "schedule-1:fire-retry", Goal: "summarise the work queue", Policy: "execute",
		Retention: activities.RetentionDurable,
	}
	env.OnActivity(
		"StartScheduledRunActivity",
		mock.Anything, input.RunID, input.ThreadID, input.OrgID, input.SpaceRef, input.SubjectID,
		input.ScheduleID, input.FireKey, input.TemplateDigest, input.IdempotencyKey, input.Goal, "execute",
	).Return(activities.RunMetadata{
		RunID: input.RunID, ThreadID: input.ThreadID, OrgID: input.OrgID,
		UserID: activities.SystemActorID,
	}, nil).Once()

	var seen []activities.ScheduledStepExecutionIntent
	stepMatcher := mock.MatchedBy(func(intent activities.ScheduledStepExecutionIntent) bool {
		return intent.RunID == input.RunID && intent.ThreadID == input.ThreadID &&
			intent.TemplateDigest == input.TemplateDigest && intent.PolicyDigest == input.PolicyDigest
	})
	env.OnActivity("ExecuteScheduledStepActivity", mock.Anything, stepMatcher).
		Run(func(args mock.Arguments) {
			seen = append(seen, args.Get(1).(activities.ScheduledStepExecutionIntent))
		}).Return(activities.StepResult{}, errors.New("worker lost after provider submit")).Once()
	env.OnActivity("ExecuteScheduledStepActivity", mock.Anything, stepMatcher).
		Run(func(args mock.Arguments) {
			seen = append(seen, args.Get(1).(activities.ScheduledStepExecutionIntent))
		}).Return(activities.StepResult{StepIndex: 0, Completed: true, ToolName: "scheduled-step"}, nil).Once()

	env.ExecuteWorkflow(ScheduledRunSupervision, input)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Len(t, seen, 2)
	assert.Equal(t, seen[0].StepID, seen[1].StepID)
	assert.Equal(t, seen[0].IdempotencyKey, seen[1].IdempotencyKey)
	assert.Equal(t, uint32(0), seen[0].StepIndex)
	assert.Equal(t, uint32(0), seen[1].StepIndex)
	_, decoded := decodeOnlyEnvelope(t, stub.Events())
	assert.Equal(t, "RUN_COMPLETED", decoded.EventType)
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
		// An ordinary run whose issuer attested that its content is retainable.
		// Without an explicit posture the lifecycle payload omits run-derived
		// content by design — see the retention tests at the end of this file.
		Retention: activities.RetentionDurable,
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
		// An ordinary run whose issuer attested that its content is retainable.
		// Without an explicit posture the lifecycle payload omits run-derived
		// content by design — see the retention tests at the end of this file.
		Retention: activities.RetentionDurable,
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

	// Per-turn driver: the first turn needs human approval (loop breaks); after
	// the approval signal the resumed loop's first turn completes.
	firstTurn := env.OnActivity("ExecuteStepActivity", mock.Anything, mock.Anything).
		Return(activities.StepResult{StepIndex: 0, ToolName: "needs-human", NeedsApproval: true}, nil).Once()
	env.OnActivity("ExecuteStepActivity", mock.Anything, mock.Anything).
		Return(activities.StepResult{StepIndex: 0, ToolName: "final", Completed: true}, nil).NotBefore(firstTurn)

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
		// An ordinary run whose issuer attested that its content is retainable.
		// Without an explicit posture the lifecycle payload omits run-derived
		// content by design — see the retention tests at the end of this file.
		Retention: activities.RetentionDurable,
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

	env.OnActivity("ExecuteStepActivity", mock.Anything, mock.Anything).
		Return(activities.StepResult{}, errors.New("step boom"))

	input := InteractiveRunInput{
		RunID:    "run-steploop-1",
		ThreadID: "thread-1",
		Goal:     "test goal",
		Policy:   "default",
		OrgID:    "org-steploop",
		UserID:   "user-1",
		// An ordinary run whose issuer attested that its content is retainable.
		// Without an explicit posture the lifecycle payload omits run-derived
		// content by design — see the retention tests at the end of this file.
		Retention: activities.RetentionDurable,
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

	env.OnActivity("ExecuteStepActivity", mock.Anything, mock.Anything).
		Return(activities.StepResult{StepIndex: 0, ToolName: "final", Completed: true}, nil)

	env.OnActivity("CompleteRunActivity", mock.Anything, mock.Anything).
		Return(errors.New("complete boom"))

	input := InteractiveRunInput{
		RunID:    "run-complete-fail-1",
		ThreadID: "thread-1",
		Goal:     "test goal",
		Policy:   "default",
		OrgID:    "org-complete-fail",
		UserID:   "user-1",
		// An ordinary run whose issuer attested that its content is retainable.
		// Without an explicit posture the lifecycle payload omits run-derived
		// content by design — see the retention tests at the end of this file.
		Retention: activities.RetentionDurable,
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
	env.OnActivity("ExecuteStepActivity", mock.Anything, mock.Anything).
		Return(activities.StepResult{}, context.Canceled)
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
		// An ordinary run whose issuer attested that its content is retainable.
		// Without an explicit posture the lifecycle payload omits run-derived
		// content by design — see the retention tests at the end of this file.
		Retention: activities.RetentionDurable,
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

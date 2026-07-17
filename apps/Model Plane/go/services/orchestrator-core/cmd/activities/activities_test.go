package activities_test

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"sync"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/pkg/natsx"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
)

// ── Test scaffolding ─────────────────────────────────────────────────────────

type capturedEvent struct {
	subject string
	data    []byte
}

type capturingRawPublisher struct {
	mu     sync.Mutex
	events []capturedEvent
}

func (c *capturingRawPublisher) Publish(subject string, data []byte) error {
	c.mu.Lock()
	defer c.mu.Unlock()
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

func newDiscardLogger() *slog.Logger {
	return slog.New(slog.NewTextHandler(io.Discard, nil))
}

func decodeEnvelope(t *testing.T, data []byte) envelope.Envelope {
	t.Helper()
	var env envelope.Envelope
	require.NoError(t, json.Unmarshal(data, &env))
	return env
}

// ── StartRunActivity ────────────────────────────────────────────────────────

func TestStartRunActivity_NilClients_ReturnsFallbackMetadata(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)

	md, err := a.StartRunActivity(context.Background(), "run-1", "thread-1", "org-1", "user-1")

	require.NoError(t, err)
	assert.Equal(t, "run-1", md.RunID)
	assert.Equal(t, "thread-1", md.ThreadID)
	assert.Equal(t, "org-1", md.OrgID)
	assert.Equal(t, "user-1", md.UserID)
	assert.False(t, md.StartedAt.IsZero())
}

// ── ExecuteStepLoopActivity ─────────────────────────────────────────────────

func TestExecuteStepLoopActivity_NilClients_ProducesMaxTurnsPendingSteps(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)

	out, err := a.ExecuteStepLoopActivity(context.Background(), activities.StepLoopInput{
		RunID:    "r1",
		ThreadID: "t1",
		Goal:     "g",
		MaxTurns: 3,
	})

	require.NoError(t, err)
	require.Len(t, out.Steps, 3)
	for i, s := range out.Steps {
		assert.Equal(t, i, s.StepIndex)
		assert.Equal(t, "pending", s.ToolName)
		assert.False(t, s.Completed)
	}
	assert.False(t, out.Completed)
	assert.Equal(t, "executed 3 steps for run r1", out.Summary)
}

func TestExecuteStepLoopActivity_DefaultMaxTurnsTen(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)

	out, err := a.ExecuteStepLoopActivity(context.Background(), activities.StepLoopInput{
		RunID:    "r1",
		ThreadID: "t1",
		MaxTurns: 0,
	})

	require.NoError(t, err)
	assert.Len(t, out.Steps, 10)
}

func TestExecuteStepLoopActivity_ContextCancelled_ReturnsCtxErr(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)

	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	out, err := a.ExecuteStepLoopActivity(ctx, activities.StepLoopInput{
		RunID:    "r1",
		ThreadID: "t1",
		MaxTurns: 5,
	})

	require.Error(t, err)
	assert.True(t, errors.Is(err, context.Canceled))
	assert.Empty(t, out.Steps)
	assert.False(t, out.Completed)
}

// ── ExecuteStepActivity ─────────────────────────────────────────────────────

// TestExecuteStepActivity_NilClients_ReturnsPendingStep verifies the durable
// per-turn activity honors the same fallback as the legacy loop: with no
// execution-core wired it returns a non-fatal "pending" step (no error) so the
// workflow-side driver advances to the next turn, echoing the supplied
// StepIndex.
func TestExecuteStepActivity_NilClients_ReturnsPendingStep(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)

	step, err := a.ExecuteStepActivity(context.Background(), activities.StepInput{
		RunID:     "r1",
		OrgID:     "o1",
		UserID:    "u1",
		StepIndex: 4,
	})

	require.NoError(t, err)
	assert.Equal(t, 4, step.StepIndex)
	assert.Equal(t, "pending", step.ToolName)
	assert.False(t, step.Completed)
	assert.False(t, step.NeedsApproval)
}

// ── CompleteRunActivity ─────────────────────────────────────────────────────

func TestCompleteRunActivity_PublishesRunCompletedEnvelope(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)
	stub := &capturingRawPublisher{}
	a.SetPublisher(natsx.NewPublisher(stub, natsx.ModeV1Only))

	err := a.CompleteRunActivity(context.Background(), activities.CompletionInput{
		RunID:   "r1",
		OrgID:   "o1",
		UserID:  "user-1",
		Summary: "done",
	})
	require.NoError(t, err)

	events := stub.Events()
	require.Len(t, events, 1)

	assert.Equal(t, natsx.RunEventSubject("r1"), events[0].subject)
	env := decodeEnvelope(t, events[0].data)
	assert.Equal(t, "RUN_COMPLETED", env.EventType)
	assert.Equal(t, uint32(1), env.SchemaVersion)
	assert.Equal(t, "orchestrator-core", env.Producer)
	assert.Equal(t, "o1", env.OrgID)
	assert.Equal(t, "run/r1", env.ResourceRef)
	assert.Equal(t, "r1:completed", env.IdempotencyKey)
	assert.NotEmpty(t, env.EventID)

	var payload map[string]any
	require.NoError(t, json.Unmarshal(env.Payload, &payload))
	assert.Equal(t, "r1", payload["run_id"])
	assert.Equal(t, "done", payload["summary"])
}

// ── FailRunActivity ─────────────────────────────────────────────────────────

func TestFailRunActivity_PublishesRunFailedEnvelope(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)
	stub := &capturingRawPublisher{}
	a.SetPublisher(natsx.NewPublisher(stub, natsx.ModeV1Only))

	err := a.FailRunActivity(context.Background(), activities.FailureInput{
		RunID:  "r1",
		OrgID:  "o1",
		UserID: "user-1",
		Reason: "boom",
	})
	require.NoError(t, err)

	events := stub.Events()
	require.Len(t, events, 1)

	assert.Equal(t, natsx.RunEventSubject("r1"), events[0].subject)
	env := decodeEnvelope(t, events[0].data)
	assert.Equal(t, "RUN_FAILED", env.EventType)
	assert.Equal(t, "o1", env.OrgID)
	assert.Equal(t, "run/r1", env.ResourceRef)
	assert.Equal(t, "r1:failed", env.IdempotencyKey)

	var payload map[string]any
	require.NoError(t, json.Unmarshal(env.Payload, &payload))
	assert.Equal(t, "r1", payload["run_id"])
	assert.Equal(t, "boom", payload["reason"])
}

// ── Nil-publisher no-op ─────────────────────────────────────────────────────

func TestCompleteRunActivity_NilPublisher_NoOp(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)
	// publisher intentionally not set

	require.NotPanics(t, func() {
		err := a.CompleteRunActivity(context.Background(), activities.CompletionInput{
			RunID:   "r1",
			OrgID:   "o1",
			UserID:  "user-1",
			Summary: "done",
		})
		require.NoError(t, err)
	})
}

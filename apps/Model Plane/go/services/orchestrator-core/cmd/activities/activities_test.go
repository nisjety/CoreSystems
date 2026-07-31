package activities_test

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"strings"
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
		RunID:     "r1",
		OrgID:     "o1",
		UserID:    "user-1",
		Summary:   "done",
		Retention: activities.RetentionDurable,
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

// TestCompleteRunActivity_SystemRunStillPublishes is a regression test for a
// fault that silently disabled the entire learning loop: a system-initiated run
// (cron-fired task, maintenance sweep) has no acting viewer, envelope.Validate()
// requires user_id, and a validation failure means nothing is published at all.
// So every RUN_COMPLETED from a run with no user was dropped before it reached
// NATS, and capability-core's RUN_COMPLETED consumer could never fire.
func TestCompleteRunActivity_SystemRunStillPublishes(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)
	stub := &capturingRawPublisher{}
	a.SetPublisher(natsx.NewPublisher(stub, natsx.ModeV1Only))

	err := a.CompleteRunActivity(context.Background(), activities.CompletionInput{
		RunID:   "task_abc",
		OrgID:   "o1",
		UserID:  "", // org-scoped workload run: no human behind it
		Summary: "done",
	})
	require.NoError(t, err)

	events := stub.Events()
	require.Len(t, events, 1, "a system run must still emit RUN_COMPLETED")
	assert.Equal(t, natsx.RunEventSubject("task_abc"), events[0].subject)
	env := decodeEnvelope(t, events[0].data)
	assert.Equal(t, "RUN_COMPLETED", env.EventType)
	assert.Equal(t, activities.SystemActorID, env.UserID)
	require.NoError(t, env.Validate(), "the published envelope must be valid")
}

func TestFailRunActivity_SystemRunStillPublishes(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)
	stub := &capturingRawPublisher{}
	a.SetPublisher(natsx.NewPublisher(stub, natsx.ModeV1Only))

	require.NoError(t, a.FailRunActivity(context.Background(), activities.FailureInput{
		RunID: "task_abc", OrgID: "o1", Reason: "boom",
	}))
	events := stub.Events()
	require.Len(t, events, 1, "a system run must still emit RUN_FAILED")
	assert.Equal(t, activities.SystemActorID, decodeEnvelope(t, events[0].data).UserID)
}

// ── FailRunActivity ─────────────────────────────────────────────────────────

func TestFailRunActivity_PublishesRunFailedEnvelope(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)
	stub := &capturingRawPublisher{}
	a.SetPublisher(natsx.NewPublisher(stub, natsx.ModeV1Only))

	err := a.FailRunActivity(context.Background(), activities.FailureInput{
		RunID:     "r1",
		OrgID:     "o1",
		UserID:    "user-1",
		Reason:    "boom",
		Retention: activities.RetentionDurable,
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

// ── Zero Data Retention ─────────────────────────────────────────────────────

// TestRetentionPosture_MapsAttestedFlagAndGatesContent pins the tri-state:
// only an attested non-ZDR posture admits run-derived content, and the zero
// value is "unspecified" so a forgotten field fails closed.
func TestRetentionPosture_MapsAttestedFlagAndGatesContent(t *testing.T) {
	var zero activities.Retention
	assert.Equal(t, activities.RetentionUnspecified, zero, "the zero value must be the fail-closed posture")
	assert.False(t, zero.AllowsContent(), "an unspecified posture must not admit content")

	assert.Equal(t, activities.RetentionZeroData, activities.RetentionFor(true))
	assert.False(t, activities.RetentionZeroData.AllowsContent())

	assert.Equal(t, activities.RetentionDurable, activities.RetentionFor(false))
	assert.True(t, activities.RetentionDurable.AllowsContent(),
		"only an explicitly attested durable posture admits content")
}

// TestCompleteRunActivity_DurableRunDeclaresZDRFalse is the contract that makes
// the skill-learning loop able to run at all: capability-core admits ONLY an
// explicit `zdr: false` and skips on absence, so the key must actually be on the
// wire — not omitted because false is Go's zero value.
func TestCompleteRunActivity_DurableRunDeclaresZDRFalse(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)
	stub := &capturingRawPublisher{}
	a.SetPublisher(natsx.NewPublisher(stub, natsx.ModeV1Only))

	require.NoError(t, a.CompleteRunActivity(context.Background(), activities.CompletionInput{
		RunID: "r-durable", OrgID: "o1", UserID: "user-1", Summary: "done",
		Retention: activities.RetentionDurable,
	}))

	events := stub.Events()
	require.Len(t, events, 1)
	zdr, declared := envelope.DeclaredZDR(events[0].data)
	require.True(t, declared, "a durable run must DECLARE its posture, not omit the key")
	assert.False(t, zdr)
	assert.Contains(t, string(events[0].data), `"zdr":false`, "the literal wire key the consumer probes")
}

// TestCompleteRunActivity_ZDRRunEmitsNothing verifies the Go leg now matches the
// Rust DynPublisher: a ZDR envelope never reaches a backend. This matters beyond
// any single consumer because mp.v1.run.*.event is captured by the JetStream
// stream MODEL_PLANE_RUN_EVENTS (48h, file-backed), so an emitted envelope is
// retained on disk regardless of who reads it.
func TestCompleteRunActivity_ZDRRunEmitsNothing(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)
	stub := &capturingRawPublisher{}
	a.SetPublisher(natsx.NewPublisher(stub, natsx.ModeV1Only))

	require.NoError(t, a.CompleteRunActivity(context.Background(), activities.CompletionInput{
		RunID: "r-zdr", OrgID: "o1", UserID: "user-1", Summary: "customer secret",
		Retention: activities.RetentionZeroData,
	}), "a suppressed publish is not an error")

	assert.Empty(t, stub.Events(), "no ZDR envelope may reach any backend")
}

// TestCompleteRunActivity_UnattestedRunEmitsLifecycleWithoutContent covers the
// posture nobody declared. The lifecycle FACT still ships — insight-core counts
// agent_runs_completed off it — but the run's summary does not, because an
// unattested run may in truth be a ZDR run whose posture was never threaded, and
// the subject is retained for 48h.
func TestCompleteRunActivity_UnattestedRunEmitsLifecycleWithoutContent(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)
	stub := &capturingRawPublisher{}
	a.SetPublisher(natsx.NewPublisher(stub, natsx.ModeV1Only))

	require.NoError(t, a.CompleteRunActivity(context.Background(), activities.CompletionInput{
		RunID: "r-unknown", OrgID: "o1", UserID: "user-1", Summary: "customer secret",
		// Retention deliberately omitted.
	}))

	events := stub.Events()
	require.Len(t, events, 1, "the lifecycle fact must still be observable")
	assert.NotContains(t, string(events[0].data), "customer secret",
		"run content must not ride an envelope with no attested posture")

	_, declared := envelope.DeclaredZDR(events[0].data)
	assert.False(t, declared, "an unattested run must not fake a posture; absence is the signal")

	env := decodeEnvelope(t, events[0].data)
	assert.Equal(t, "RUN_COMPLETED", env.EventType)
	assert.Nil(t, env.Zdr, "absent on the wire must stay absent after decoding")
	var payload map[string]any
	require.NoError(t, json.Unmarshal(env.Payload, &payload))
	assert.Equal(t, "r-unknown", payload["run_id"])
	assert.NotContains(t, payload, "summary")
}

// TestFailRunActivity_ZDRRunEmitsNothing — the failure reason can embed
// downstream error text, so RUN_FAILED is suppressed for a ZDR run exactly like
// RUN_COMPLETED. Without this a ZDR run leaks on every unhappy path.
func TestFailRunActivity_ZDRRunEmitsNothing(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)
	stub := &capturingRawPublisher{}
	a.SetPublisher(natsx.NewPublisher(stub, natsx.ModeV1Only))

	require.NoError(t, a.FailRunActivity(context.Background(), activities.FailureInput{
		RunID: "r-zdr", OrgID: "o1", UserID: "user-1", Reason: "step loop: leaked detail",
		Retention: activities.RetentionZeroData,
	}))

	assert.Empty(t, stub.Events())
}

func TestFailRunActivity_UnattestedRunOmitsReason(t *testing.T) {
	a := activities.NewActivities(newDiscardLogger(), nil)
	stub := &capturingRawPublisher{}
	a.SetPublisher(natsx.NewPublisher(stub, natsx.ModeV1Only))

	require.NoError(t, a.FailRunActivity(context.Background(), activities.FailureInput{
		RunID: "r-unknown", OrgID: "o1", UserID: "user-1", Reason: "step loop: leaked detail",
	}))

	events := stub.Events()
	require.Len(t, events, 1)
	assert.NotContains(t, string(events[0].data), "leaked detail")
	env := decodeEnvelope(t, events[0].data)
	assert.Equal(t, "RUN_FAILED", env.EventType)
	var payload map[string]any
	require.NoError(t, json.Unmarshal(env.Payload, &payload))
	assert.NotContains(t, payload, "reason")
}

// TestPublishEvalRoundActivity_RetentionGatesJudgeFeedback — the judge's critique
// is model output derived from the run, so it obeys the same rule as the summary.
func TestPublishEvalRoundActivity_RetentionGatesJudgeFeedback(t *testing.T) {
	for _, tc := range []struct {
		name        string
		retention   activities.Retention
		wantEvents  int
		wantContent bool
	}{
		{"zdr suppresses the whole envelope", activities.RetentionZeroData, 0, false},
		{"unattested emits without feedback", activities.RetentionUnspecified, 1, false},
		{"durable emits with feedback", activities.RetentionDurable, 1, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			a := activities.NewActivities(newDiscardLogger(), nil)
			stub := &capturingRawPublisher{}
			a.SetPublisher(natsx.NewPublisher(stub, natsx.ModeV1Only))

			require.NoError(t, a.PublishEvalRoundActivity(context.Background(), activities.EvalRoundEvent{
				RunID: "r-eval", OrgID: "o1", UserID: "user-1", Round: 1,
				Feedback: "verdict-detail", Retention: tc.retention,
			}))

			events := stub.Events()
			require.Len(t, events, tc.wantEvents)
			if tc.wantEvents == 0 {
				return
			}
			assert.Equal(t, tc.wantContent, strings.Contains(string(events[0].data), "verdict-detail"))
		})
	}
}

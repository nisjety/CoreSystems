package taskexec

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/triodelab/model-plane/pkg/envelope"
)

func runEnvelope(t *testing.T, eventType, orgID, runID, reason string) []byte {
	t.Helper()
	payload := map[string]string{"run_id": runID}
	if reason != "" {
		payload["reason"] = reason
	}
	raw, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	env := envelope.Envelope{
		EventType:     eventType,
		OrgID:         orgID,
		CorrelationID: runID,
		Payload:       raw,
	}
	data, err := json.Marshal(env)
	if err != nil {
		t.Fatalf("marshal envelope: %v", err)
	}
	return data
}

func TestTerminalStatusMapping(t *testing.T) {
	cases := map[string]string{
		"RUN_COMPLETED": "completed",
		"RUN_FAILED":    "failed",
	}
	for eventType, want := range cases {
		got, _, ok := terminalStatus(eventType)
		if !ok || got != want {
			t.Fatalf("terminalStatus(%q) = (%q,%v), want (%q,true)", eventType, got, ok, want)
		}
	}
	for _, eventType := range []string{"RUN_STARTED", "STEP_COMPLETED", "", "run_completed"} {
		if _, _, ok := terminalStatus(eventType); ok {
			t.Fatalf("terminalStatus(%q) must not be terminal", eventType)
		}
	}
}

// TestHandleIgnoresNonTaskRuns is the guard that keeps ordinary chat runs — which
// share this subject — from ever touching the tasks table. Reaching the database
// would panic on the nil pool, so a nil-pool consumer proves the short-circuit.
func TestHandleIgnoresNonTaskRuns(t *testing.T) {
	c := &RunCompletionConsumer{pool: nil}
	for _, data := range [][]byte{
		runEnvelope(t, "RUN_COMPLETED", "org_1", "01KYTKCW6E7TZH691SV90XCFD9", ""),
		runEnvelope(t, "RUN_STARTED", "org_1", "task_abc", ""),
		runEnvelope(t, "STEP_COMPLETED", "org_1", "task_abc", ""),
	} {
		closed, err := c.Handle(context.Background(), data)
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if closed != "" {
			t.Fatalf("expected no task closed, got %q", closed)
		}
	}
}

func TestHandleRejectsMalformedEnvelope(t *testing.T) {
	c := &RunCompletionConsumer{pool: nil}
	if _, err := c.Handle(context.Background(), []byte("{not json")); err == nil {
		t.Fatal("a malformed envelope must be an error")
	}
}

// TestHandleRequiresAnOrgOnTerminalTaskEvents pins the tenant guard: the org is
// the only thing stopping one tenant's run event from closing another tenant's
// task, so an envelope with no org is refused before any SQL runs.
func TestHandleRequiresAnOrgOnTerminalTaskEvents(t *testing.T) {
	c := &RunCompletionConsumer{pool: nil}
	if _, err := c.Handle(context.Background(),
		runEnvelope(t, "RUN_COMPLETED", "", "task_abc", "")); err == nil {
		t.Fatal("a terminal task event with no org_id must be refused")
	}
}

func TestRunIDFallsBackToCorrelationID(t *testing.T) {
	env := &envelope.Envelope{CorrelationID: "task_xyz"}
	if got := runIDFrom(env); got != "task_xyz" {
		t.Fatalf("runIDFrom = %q, want task_xyz", got)
	}
	env.Payload = json.RawMessage(`{"run_id":"task_from_payload"}`)
	if got := runIDFrom(env); got != "task_from_payload" {
		t.Fatalf("runIDFrom = %q, want task_from_payload", got)
	}
}

func TestFailureReasonExtraction(t *testing.T) {
	if got := failureReason(json.RawMessage(`{"reason":"start run: unauthenticated"}`)); got != "start run: unauthenticated" {
		t.Fatalf("failureReason = %q", got)
	}
	if got := failureReason(nil); got != "" {
		t.Fatalf("failureReason(nil) = %q, want empty", got)
	}
	if got := failureReason(json.RawMessage(`not json`)); got != "" {
		t.Fatalf("failureReason(bad) = %q, want empty", got)
	}
}

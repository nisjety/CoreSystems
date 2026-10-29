package mpv1

import (
	"encoding/hex"
	"testing"

	"google.golang.org/protobuf/proto"
)

// Cross-language wire-compat goldens for OrchestrationEvent.
// These hex strings MUST match byte-for-byte the GOLDEN_* constants in:
//   rust/crates/mp-orchestration/tests/proto_wire_parity.rs
// All variants share Timestamp at = 1_700_000_000s (prefix 0a060880e2cfaa06).

const (
	goldenOrchPlanTransitionedHex        = "0a060880e2cfaa0652130a06706c616e2d31120572756e2d3118012002"
	goldenOrchTodoTransitionedHex        = "0a060880e2cfaa065a160a06746f646f2d3112087468726561642d3118012002"
	goldenOrchApprovalStateChangedHex    = "0a060880e2cfaa0662250a06617070722d31120572756e2d31180220022a1075736572406578616d706c652e636f6d"
	goldenOrchSubagentAttachedHex        = "0a060880e2cfaa066a190a0a72756e2d706172656e74120972756e2d6368696c641802"
	goldenOrchSubagentStoppedHex         = "0a060880e2cfaa0672160a0972756e2d6368696c641209636f6d706c65746564"
	goldenOrchRunPausedForApprovalHex    = "0a060880e2cfaa067a0f0a0572756e2d311206617070722d31"
	goldenOrchRunResumedAfterApprovalHex = "0a060880e2cfaa0682010f0a0572756e2d311206617070722d31"
)

const expectedAtSeconds int64 = 1_700_000_000

func decodeOrchHex(t *testing.T, h string) *OrchestrationEvent {
	t.Helper()
	raw, err := hex.DecodeString(h)
	if err != nil {
		t.Fatalf("hex decode: %v", err)
	}
	ev := &OrchestrationEvent{}
	if err := proto.Unmarshal(raw, ev); err != nil {
		t.Fatalf("proto.Unmarshal: %v", err)
	}
	if got := ev.GetAt().GetSeconds(); got != expectedAtSeconds {
		t.Fatalf("at.seconds = %d, want %d", got, expectedAtSeconds)
	}
	return ev
}

func TestOrchestrationEventDecodePlanTransitioned(t *testing.T) {
	ev := decodeOrchHex(t, goldenOrchPlanTransitionedHex)
	v, ok := ev.GetEvent().(*OrchestrationEvent_PlanTransitioned_)
	if !ok {
		t.Fatalf("variant = %T, want *OrchestrationEvent_PlanTransitioned_", ev.GetEvent())
	}
	p := v.PlanTransitioned
	if p.GetPlanId() != "plan-1" || p.GetRunId() != "run-1" {
		t.Fatalf("ids = %q/%q", p.GetPlanId(), p.GetRunId())
	}
	if int32(p.GetFrom()) != 1 || int32(p.GetTo()) != 2 {
		t.Fatalf("from/to = %d/%d", p.GetFrom(), p.GetTo())
	}
}

func TestOrchestrationEventDecodeTodoTransitioned(t *testing.T) {
	ev := decodeOrchHex(t, goldenOrchTodoTransitionedHex)
	v, ok := ev.GetEvent().(*OrchestrationEvent_TodoTransitioned_)
	if !ok {
		t.Fatalf("variant = %T", ev.GetEvent())
	}
	p := v.TodoTransitioned
	if p.GetTodoId() != "todo-1" || p.GetThreadId() != "thread-1" {
		t.Fatalf("ids = %q/%q", p.GetTodoId(), p.GetThreadId())
	}
	if int32(p.GetFrom()) != 1 || int32(p.GetTo()) != 2 {
		t.Fatalf("from/to = %d/%d", p.GetFrom(), p.GetTo())
	}
}

func TestOrchestrationEventDecodeApprovalStateChanged(t *testing.T) {
	ev := decodeOrchHex(t, goldenOrchApprovalStateChangedHex)
	v, ok := ev.GetEvent().(*OrchestrationEvent_ApprovalStateChanged_)
	if !ok {
		t.Fatalf("variant = %T", ev.GetEvent())
	}
	p := v.ApprovalStateChanged
	if p.GetApprovalId() != "appr-1" || p.GetRunId() != "run-1" {
		t.Fatalf("ids = %q/%q", p.GetApprovalId(), p.GetRunId())
	}
	if int32(p.GetApprovalKind()) != 2 || int32(p.GetTo()) != 2 {
		t.Fatalf("kind/to = %d/%d", p.GetApprovalKind(), p.GetTo())
	}
	if p.GetDecidedBy() != "user@example.com" {
		t.Fatalf("decided_by = %q", p.GetDecidedBy())
	}
}

func TestOrchestrationEventDecodeSubagentAttached(t *testing.T) {
	ev := decodeOrchHex(t, goldenOrchSubagentAttachedHex)
	v, ok := ev.GetEvent().(*OrchestrationEvent_SubagentAttached_)
	if !ok {
		t.Fatalf("variant = %T", ev.GetEvent())
	}
	p := v.SubagentAttached
	if p.GetParentRunId() != "run-parent" || p.GetChildRunId() != "run-child" {
		t.Fatalf("ids = %q/%q", p.GetParentRunId(), p.GetChildRunId())
	}
	if int32(p.GetRole()) != 2 {
		t.Fatalf("role = %d", p.GetRole())
	}
}

func TestOrchestrationEventDecodeSubagentStopped(t *testing.T) {
	ev := decodeOrchHex(t, goldenOrchSubagentStoppedHex)
	v, ok := ev.GetEvent().(*OrchestrationEvent_SubagentStopped_)
	if !ok {
		t.Fatalf("variant = %T", ev.GetEvent())
	}
	p := v.SubagentStopped
	if p.GetChildRunId() != "run-child" || p.GetStatus() != "completed" {
		t.Fatalf("fields = %q/%q", p.GetChildRunId(), p.GetStatus())
	}
}

func TestOrchestrationEventDecodeRunPausedForApproval(t *testing.T) {
	ev := decodeOrchHex(t, goldenOrchRunPausedForApprovalHex)
	v, ok := ev.GetEvent().(*OrchestrationEvent_RunPausedForApproval_)
	if !ok {
		t.Fatalf("variant = %T", ev.GetEvent())
	}
	p := v.RunPausedForApproval
	if p.GetRunId() != "run-1" || p.GetApprovalId() != "appr-1" {
		t.Fatalf("ids = %q/%q", p.GetRunId(), p.GetApprovalId())
	}
}

func TestOrchestrationEventDecodeRunResumedAfterApproval(t *testing.T) {
	ev := decodeOrchHex(t, goldenOrchRunResumedAfterApprovalHex)
	v, ok := ev.GetEvent().(*OrchestrationEvent_RunResumedAfterApproval_)
	if !ok {
		t.Fatalf("variant = %T", ev.GetEvent())
	}
	p := v.RunResumedAfterApproval
	if p.GetRunId() != "run-1" || p.GetApprovalId() != "appr-1" {
		t.Fatalf("ids = %q/%q", p.GetRunId(), p.GetApprovalId())
	}
}

func TestOrchestrationEventRoundTripReencodesIdentically(t *testing.T) {
	cases := []string{
		goldenOrchPlanTransitionedHex,
		goldenOrchTodoTransitionedHex,
		goldenOrchApprovalStateChangedHex,
		goldenOrchSubagentAttachedHex,
		goldenOrchSubagentStoppedHex,
		goldenOrchRunPausedForApprovalHex,
		goldenOrchRunResumedAfterApprovalHex,
	}
	for _, h := range cases {
		raw, err := hex.DecodeString(h)
		if err != nil {
			t.Fatalf("hex decode %s: %v", h, err)
		}
		ev := &OrchestrationEvent{}
		if err := proto.Unmarshal(raw, ev); err != nil {
			t.Fatalf("unmarshal %s: %v", h, err)
		}
		out, err := proto.MarshalOptions{Deterministic: true}.Marshal(ev)
		if err != nil {
			t.Fatalf("marshal %s: %v", h, err)
		}
		if got := hex.EncodeToString(out); got != h {
			t.Fatalf("re-encode mismatch:\n want %s\n got  %s", h, got)
		}
	}
}

func TestOrchestrationEventDecodeRejectsTruncatedPayload(t *testing.T) {
	raw, err := hex.DecodeString(goldenOrchApprovalStateChangedHex)
	if err != nil {
		t.Fatalf("hex decode: %v", err)
	}
	truncated := raw[:len(raw)-3]
	ev := &OrchestrationEvent{}
	if err := proto.Unmarshal(truncated, ev); err == nil {
		t.Fatalf("expected error on truncated payload, got nil")
	}
}

package runwatch

import (
	"context"
	"encoding/json"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/envelope"
)

// attentionEnvelopeWithReason encodes a RUN_ATTENTION on thread-7 for run-park-1 with the given reason (empty for a
// pending row) and state. The source is the row the item is about.
func attentionEnvelopeWithReason(t *testing.T, reason, state, sourceID string) []byte {
	t.Helper()
	payload := map[string]string{
		"run_id":    "run-park-1",
		"source_id": sourceID,
		"state":     state,
		"thread_id": "thread-7",
	}
	if reason != "" {
		payload["reason"] = reason
	}
	raw, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("encode payload: %v", err)
	}
	env := envelope.Envelope{
		EventID:       "run_attention:session-core:" + sourceID,
		EventType:     runAttentionEventType,
		SchemaVersion: 1,
		Ts:            attentionFixedTime,
		Producer:      "session-core",
		CorrelationID: "run-park-1",
		CausationID:   sourceID,
		OrgID:         "org-1",
		UserID:        "user-1",
		ResourceRef:   "run:run-park-1",
		Payload:       json.RawMessage(raw),
		Zdr:           envelope.ZDRFlag(false),
	}
	data, err := env.Encode()
	if err != nil {
		t.Fatalf("encode envelope: %v", err)
	}
	return data
}

// TestAttentionNotifierParkedRunIsNotifiedAsContentFreeNeedsInput pins the O-02 notice: a crash-parked run is
// announced once, as NEEDS_INPUT, with its own copy and the thread's deep link, and carries no run content.
func TestAttentionNotifierParkedRunIsNotifiedAsContentFreeNeedsInput(t *testing.T) {
	notify := &fakeNotifyClient{}
	n := newAttentionNotifier(&fakeStore{}, notify, 0, fixedNow)

	data := attentionEnvelopeWithReason(t, "ATTENTION_REASON_PARKED", "ATTENTION_STATE_NEEDS_INPUT", "park-ref-1")
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 1 {
		t.Fatalf("notify calls = %d, want 1", len(notify.accepted))
	}
	req := notify.accepted[0]
	if req.Type != "modelplane.needs_input" || req.RetentionMode != "standard" {
		t.Fatalf("request = %+v, want modelplane.needs_input with standard retention", req)
	}
	const wantKey = "attention:org-1:run-park-1:user-1:ATTENTION_REASON_PARKED:park-ref-1"
	if req.IdempotencyKey != wantKey {
		t.Fatalf("idempotency key = %q, want %q", req.IdempotencyKey, wantKey)
	}
	if req.Payload["kind"] != "parked" || req.Payload["message_key"] != "modelplane.needs_input" ||
		req.Payload["cta_href"] != "/chat?thread=thread-7" {
		t.Fatalf("payload = %+v, want the parked kind and the thread deep link", req.Payload)
	}
	assertContentFree(t, req.Payload)
}

// TestAttentionNotifierParkedNotifiesNothingButNeedsInput: a parked reason names one state only. Any other state with
// the reason is not a parked run, so it is acknowledged without a notification.
func TestAttentionNotifierParkedNotifiesNothingButNeedsInput(t *testing.T) {
	for _, state := range []string{
		"ATTENTION_STATE_NEEDS_APPROVAL",
		"ATTENTION_STATE_PLAN_READY",
		"ATTENTION_STATE_WORKING",
		"ATTENTION_STATE_COMPLETED",
	} {
		notify := &fakeNotifyClient{}
		n := newAttentionNotifier(&fakeStore{}, notify, 0, fixedNow)
		data := attentionEnvelopeWithReason(t, "ATTENTION_REASON_PARKED", state, "park-ref-1")
		if got := n.process(context.Background(), data); got != outcomeAck {
			t.Fatalf("%s outcome = %v, want outcomeAck", state, got)
		}
		if len(notify.accepted) != 0 {
			t.Fatalf("%s notify calls = %d, want 0", state, len(notify.accepted))
		}
	}
}

// TestAttentionNotifierAnUnknownReasonIsIgnored: a reason this build does not name is never routed to a template,
// whatever its state, so it notifies nothing.
func TestAttentionNotifierAnUnknownReasonIsIgnored(t *testing.T) {
	for _, state := range []string{"ATTENTION_STATE_NEEDS_INPUT", "ATTENTION_STATE_NEEDS_APPROVAL"} {
		notify := &fakeNotifyClient{}
		n := newAttentionNotifier(&fakeStore{}, notify, 0, fixedNow)
		data := attentionEnvelopeWithReason(t, "ATTENTION_REASON_SOMETHING_NEW", state, "item-1")
		if got := n.process(context.Background(), data); got != outcomeAck {
			t.Fatalf("%s outcome = %v, want outcomeAck", state, got)
		}
		if len(notify.accepted) != 0 {
			t.Fatalf("%s notify calls = %d, want 0", state, len(notify.accepted))
		}
	}
}

// TestAttentionNotifierAParkedRunDoesNotFoldIntoAQuestionOnTheSameThread: a question and a parked run on one thread,
// inside one coalescing window, are two notifications. Each keys on its own reason or state.
func TestAttentionNotifierAParkedRunDoesNotFoldIntoAQuestionOnTheSameThread(t *testing.T) {
	notify := &fakeNotifyClient{}
	n := newAttentionNotifier(&fakeStore{}, notify, time.Minute, fixedNow)

	question := attentionEnvelopeWithReason(t, "", "ATTENTION_STATE_NEEDS_INPUT", "question-1")
	if got := n.process(context.Background(), question); got != outcomeAck {
		t.Fatalf("question outcome = %v, want outcomeAck", got)
	}
	parked := attentionEnvelopeWithReason(t, "ATTENTION_REASON_PARKED", "ATTENTION_STATE_NEEDS_INPUT", "park-ref-1")
	if got := n.process(context.Background(), parked); got != outcomeAck {
		t.Fatalf("parked outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 2 {
		t.Fatalf("notify calls = %d, want 2: the parked run does not fold into the question", len(notify.accepted))
	}
	if notify.accepted[0].Payload["kind"] != "needs_input" || notify.accepted[1].Payload["kind"] != "parked" {
		t.Fatalf("kinds = %v, %v, want needs_input then parked", notify.accepted[0].Payload["kind"], notify.accepted[1].Payload["kind"])
	}
}

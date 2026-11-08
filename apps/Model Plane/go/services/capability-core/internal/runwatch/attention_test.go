package runwatch

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/notifyclient"
)

// goldenRunAttention is session-core's pinned RUN_ATTENTION wire body (WS16-S3).
// Decoding the producer's bytes, rather than a shape written here, is what makes
// this a cross-service check.
const goldenRunAttention = "../../../../../api/contracts/run-attention-envelope.json"

// closedAttentionPayloadKeys is the only field set an attention notification may
// carry. notification-core's native path refuses any other field, so a field added
// here must be added to its closed set as well.
var closedAttentionPayloadKeys = []string{"title", "body", "message_key", "cta_href", "cta_label", "thread_id", "run_id", "kind"}

var attentionFixedTime = time.Date(2026, 8, 18, 1, 0, 0, 0, time.UTC)

func fixedNow() time.Time { return attentionFixedTime }

// attentionEnvelope encodes one RUN_ATTENTION the way session-core writes it: ids and one closed state.
func attentionEnvelope(t *testing.T, state, sourceID, threadID string, zdr *bool) []byte {
	t.Helper()
	env := envelope.Envelope{
		EventID:       "run_attention:session-core:test-" + sourceID,
		EventType:     runAttentionEventType,
		SchemaVersion: 1,
		Ts:            attentionFixedTime,
		Producer:      "session-core",
		CorrelationID: "run-abc",
		CausationID:   sourceID,
		OrgID:         "org-1",
		UserID:        "user-1",
		ResourceRef:   "run:run-abc",
		Payload: json.RawMessage(`{"run_id":"run-abc","source_id":"` + sourceID +
			`","state":"` + state + `","thread_id":"` + threadID + `"}`),
		Zdr: zdr,
	}
	data, err := env.Encode()
	if err != nil {
		t.Fatalf("encode envelope: %v", err)
	}
	return data
}

func newAttentionNotifier(store SubscriptionStore, notify NotifyClient, coalesce time.Duration, now func() time.Time) *Notifier {
	n := &Notifier{store: store, notify: notify, now: now}
	WithAttentionNotify(coalesce)(n)
	return n
}

func assertContentFree(t *testing.T, payload map[string]any) {
	t.Helper()
	allowed := make(map[string]bool, len(closedAttentionPayloadKeys))
	for _, key := range closedAttentionPayloadKeys {
		allowed[key] = true
	}
	if len(payload) != len(allowed) {
		t.Fatalf("payload has %d fields, want the %d content-free fields", len(payload), len(allowed))
	}
	for key := range payload {
		if !allowed[key] {
			t.Fatalf("payload carries %q, which is outside the content-free set", key)
		}
	}
}

func TestRunAttentionGoldenDecodesToOneNeedsApprovalNotification(t *testing.T) {
	golden, err := os.ReadFile(goldenRunAttention)
	if err != nil {
		t.Fatalf("read golden envelope: %v", err)
	}
	store := &fakeStore{}
	notify := &fakeNotifyClient{}
	n := newAttentionNotifier(store, notify, time.Minute, fixedNow)

	if got := n.process(context.Background(), golden); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if store.lookups != 0 {
		t.Fatal("an attention event must not read run watchers")
	}
	if len(notify.accepted) != 1 {
		t.Fatalf("notify calls = %d, want 1", len(notify.accepted))
	}
	req := notify.accepted[0]
	if req.OrganizationID != "org-1" || req.Type != "modelplane.needs_approval" || req.RetentionMode != "standard" {
		t.Fatalf("request = %+v, want org-1, modelplane.needs_approval, standard retention", req)
	}
	if req.Recipient.Kind != notifyclient.RecipientKindUser || req.Recipient.ID != "user-1" {
		t.Fatalf("recipient = %+v, want the envelope's user", req.Recipient)
	}
	const wantKey = "attention:org-1:run-abc:user-1:ATTENTION_STATE_NEEDS_APPROVAL:apr-1"
	if req.IdempotencyKey != wantKey {
		t.Fatalf("idempotency key = %q, want %q", req.IdempotencyKey, wantKey)
	}
	if req.Payload["thread_id"] != "thread-9" || req.Payload["cta_href"] != "/chat?thread=thread-9" {
		t.Fatalf("payload = %+v, want the thread deep link", req.Payload)
	}
	assertContentFree(t, req.Payload)
}

func TestRunAttentionMapsEachWaitingStateToItsNotificationType(t *testing.T) {
	for state, want := range map[string]struct{ notifyType, kind string }{
		"ATTENTION_STATE_NEEDS_APPROVAL": {"modelplane.needs_approval", "needs_approval"},
		"ATTENTION_STATE_NEEDS_INPUT":    {"modelplane.needs_input", "needs_input"},
		"ATTENTION_STATE_PLAN_READY":     {"modelplane.plan_ready", "plan_ready"},
	} {
		store := &fakeStore{}
		notify := &fakeNotifyClient{}
		n := newAttentionNotifier(store, notify, 0, fixedNow)

		data := attentionEnvelope(t, state, "src-1", "thread-9", envelope.ZDRFlag(false))
		if got := n.process(context.Background(), data); got != outcomeAck {
			t.Fatalf("state %s outcome = %v, want outcomeAck", state, got)
		}
		if len(notify.accepted) != 1 {
			t.Fatalf("state %s notify calls = %d, want 1", state, len(notify.accepted))
		}
		if notify.accepted[0].Type != want.notifyType {
			t.Fatalf("state %s type = %q, want %q", state, notify.accepted[0].Type, want.notifyType)
		}
		if notify.accepted[0].Payload["kind"] != want.kind || notify.accepted[0].Payload["message_key"] != want.notifyType {
			t.Fatalf("state %s payload = %+v, want kind %q and message_key %q", state, notify.accepted[0].Payload, want.kind, want.notifyType)
		}
		assertContentFree(t, notify.accepted[0].Payload)
	}
}

func TestRunAttentionIsRefusedUnlessTheRunDeclaresPersistent(t *testing.T) {
	for name, zdr := range map[string]*bool{
		"posture undeclared": nil,
		"zero retention":     envelope.ZDRFlag(true),
	} {
		t.Run(name, func(t *testing.T) {
			store := &fakeStore{}
			notify := &fakeNotifyClient{}
			n := newAttentionNotifier(store, notify, time.Minute, fixedNow)

			data := attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", zdr)
			if got := n.process(context.Background(), data); got != outcomeAck {
				t.Fatalf("outcome = %v, want outcomeAck", got)
			}
			if len(notify.accepted) != 0 || store.lookups != 0 {
				t.Fatalf("accepted = %d, lookups = %d, want nothing sent and no lookup", len(notify.accepted), store.lookups)
			}
		})
	}
}

func TestRunAttentionIsAckedLikeAnyEventWhileTheSwitchIsOff(t *testing.T) {
	store := &fakeStore{watchers: map[string][]Watcher{"org-1/run-abc": {{ID: "watch-a", UserID: "user-1"}}}}
	notify := &fakeNotifyClient{}
	n := &Notifier{store: store, notify: notify, now: fixedNow}

	data := attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 0 || store.lookups != 0 {
		t.Fatalf("accepted = %d, lookups = %d, want the event ignored as before", len(notify.accepted), store.lookups)
	}
}

func TestRunAttentionIgnoresStatesThatDoNotWait(t *testing.T) {
	for _, state := range []string{"ATTENTION_STATE_WORKING", "ATTENTION_STATE_COMPLETED", "ATTENTION_STATE_IDLE", "ATTENTION_STATE_UNSPECIFIED"} {
		notify := &fakeNotifyClient{}
		n := newAttentionNotifier(&fakeStore{}, notify, time.Minute, fixedNow)

		data := attentionEnvelope(t, state, "src-1", "thread-9", envelope.ZDRFlag(false))
		if got := n.process(context.Background(), data); got != outcomeAck {
			t.Fatalf("state %s outcome = %v, want outcomeAck", state, got)
		}
		if len(notify.accepted) != 0 {
			t.Fatalf("state %s notified, want no notification", state)
		}
	}
}

func TestRunAttentionWithoutIdentifiersIsAcked(t *testing.T) {
	for name, payload := range map[string]string{
		"no thread": `{"run_id":"run-abc","source_id":"apr-1","state":"ATTENTION_STATE_NEEDS_APPROVAL"}`,
		"no source": `{"run_id":"run-abc","state":"ATTENTION_STATE_NEEDS_APPROVAL","thread_id":"thread-9"}`,
	} {
		t.Run(name, func(t *testing.T) {
			env := envelope.Envelope{
				EventType:     runAttentionEventType,
				OrgID:         "org-1",
				UserID:        "user-1",
				CorrelationID: "run-abc",
				Payload:       json.RawMessage(payload),
				Zdr:           envelope.ZDRFlag(false),
			}
			data, err := env.Encode()
			if err != nil {
				t.Fatal(err)
			}
			notify := &fakeNotifyClient{}
			n := newAttentionNotifier(&fakeStore{}, notify, time.Minute, fixedNow)
			if got := n.process(context.Background(), data); got != outcomeAck {
				t.Fatalf("outcome = %v, want outcomeAck", got)
			}
			if len(notify.accepted) != 0 {
				t.Fatal("an event missing an identifier must not notify")
			}
		})
	}
}

func TestRunAttentionCoalescesRepeatsOfOneThreadAndStateWithinTheWindow(t *testing.T) {
	clock := attentionFixedTime
	now := func() time.Time { return clock }
	notify := &fakeNotifyClient{}
	n := newAttentionNotifier(&fakeStore{}, notify, time.Minute, now)

	if got := n.process(context.Background(), attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false))); got != outcomeAck {
		t.Fatalf("first outcome = %v, want outcomeAck", got)
	}

	clock = attentionFixedTime.Add(10 * time.Second)
	if got := n.process(context.Background(), attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-2", "thread-9", envelope.ZDRFlag(false))); got != outcomeAck {
		t.Fatalf("second outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 1 {
		t.Fatalf("notify calls inside the window = %d, want 1", len(notify.accepted))
	}

	clock = attentionFixedTime.Add(61 * time.Second)
	if got := n.process(context.Background(), attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-3", "thread-9", envelope.ZDRFlag(false))); got != outcomeAck {
		t.Fatalf("third outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 2 {
		t.Fatalf("notify calls after the window = %d, want 2", len(notify.accepted))
	}
	if notify.accepted[0].IdempotencyKey == notify.accepted[1].IdempotencyKey {
		t.Fatal("two waiting items must not share one idempotency key")
	}
}

func TestRunAttentionCoalescingIsPerThreadAndPerState(t *testing.T) {
	notify := &fakeNotifyClient{}
	n := newAttentionNotifier(&fakeStore{}, notify, time.Minute, fixedNow)

	_ = n.process(context.Background(), attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false)))
	_ = n.process(context.Background(), attentionEnvelope(t, "ATTENTION_STATE_NEEDS_INPUT", "q-1", "thread-9", envelope.ZDRFlag(false)))
	_ = n.process(context.Background(), attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-2", "thread-10", envelope.ZDRFlag(false)))

	if len(notify.accepted) != 3 {
		t.Fatalf("notify calls = %d, want 3: another state or another thread is its own stream", len(notify.accepted))
	}
}

func TestRunAttentionFailedDelegationIsRetriedAndNotCoalesced(t *testing.T) {
	notify := &fakeNotifyClient{failFor: map[string]error{"user-1": errors.New("notification-core unreachable")}}
	n := newAttentionNotifier(&fakeStore{}, notify, time.Minute, fixedNow)
	data := attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false))

	if got := n.process(context.Background(), data); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry when delegation fails", got)
	}

	notify.failFor = nil
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("redelivery outcome = %v, want outcomeAck once delegation recovers", got)
	}
	if len(notify.accepted) != 1 {
		t.Fatalf("notify calls = %d, want the redelivery to notify exactly once", len(notify.accepted))
	}
}

func TestRunAttentionWithoutCoalescingNotifiesEveryWaitingItem(t *testing.T) {
	notify := &fakeNotifyClient{}
	n := newAttentionNotifier(&fakeStore{}, notify, 0, fixedNow)

	_ = n.process(context.Background(), attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false)))
	_ = n.process(context.Background(), attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-2", "thread-9", envelope.ZDRFlag(false)))

	if len(notify.accepted) != 2 {
		t.Fatalf("notify calls = %d, want 2 with coalescing off", len(notify.accepted))
	}
}

func TestRunCompletedOfADeclaredZeroRetentionRunIsAckedWithoutLookup(t *testing.T) {
	store := &fakeStore{watchers: map[string][]Watcher{"org-1/run-1": {{ID: "watch-a", UserID: "user-a"}}}}
	notify := &fakeNotifyClient{}
	n := &Notifier{store: store, notify: notify, now: fixedNow}

	zdrEnv := envelope.Envelope{EventType: "RUN_COMPLETED", OrgID: "org-1", UserID: "user-system", CorrelationID: "run-1", Payload: json.RawMessage(`{"run_id":"run-1"}`), Zdr: envelope.ZDRFlag(true)}
	data, err := zdrEnv.Encode()
	if err != nil {
		t.Fatal(err)
	}
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("zdr outcome = %v, want outcomeAck", got)
	}
	if store.lookups != 0 || len(notify.accepted) != 0 {
		t.Fatalf("zdr run: lookups = %d, accepted = %d, want no lookup and no notification", store.lookups, len(notify.accepted))
	}

	persistentEnv := zdrEnv
	persistentEnv.Zdr = envelope.ZDRFlag(false)
	data, err = persistentEnv.Encode()
	if err != nil {
		t.Fatal(err)
	}
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("persistent outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 1 {
		t.Fatalf("persistent run_completed notify calls = %d, want the watcher still notified", len(notify.accepted))
	}
}

func TestParseAttentionCoalesce(t *testing.T) {
	for value, want := range map[string]time.Duration{
		"":     DefaultAttentionCoalesce,
		"abc":  DefaultAttentionCoalesce,
		"-5":   DefaultAttentionCoalesce,
		"0":    0,
		"30":   30 * time.Second,
		" 45 ": 45 * time.Second,
	} {
		if got := ParseAttentionCoalesce(value); got != want {
			t.Fatalf("ParseAttentionCoalesce(%q) = %v, want %v", value, got, want)
		}
	}
}

// goalAttentionEnvelope encodes a RUN_ATTENTION for a stopped thread goal (EP-7
// S5): the pending-row shape plus the goal reason and one closed status token.
func goalAttentionEnvelope(t *testing.T, state, goalStatus, goalID string) []byte {
	t.Helper()
	env := envelope.Envelope{
		EventID:       "run_attention:session-core:test-" + goalID,
		EventType:     runAttentionEventType,
		SchemaVersion: 1,
		Ts:            attentionFixedTime,
		Producer:      "session-core",
		CorrelationID: "run-abc",
		CausationID:   goalID,
		OrgID:         "org-1",
		UserID:        "user-1",
		ResourceRef:   "run:run-abc",
		Payload: json.RawMessage(`{"goal_status":"` + goalStatus + `","reason":"ATTENTION_REASON_GOAL","run_id":"run-abc","source_id":"` +
			goalID + `","state":"` + state + `","thread_id":"thread-9"}`),
		Zdr: envelope.ZDRFlag(false),
	}
	data, err := env.Encode()
	if err != nil {
		t.Fatalf("encode envelope: %v", err)
	}
	return data
}

func TestAttentionNotifierGoalStoppedIsContentFree(t *testing.T) {
	for goalStatus, state := range map[string]string{
		"met":            "ATTENTION_STATE_COMPLETED",
		"budget_reached": "ATTENTION_STATE_NEEDS_INPUT",
		"turns_reached":  "ATTENTION_STATE_NEEDS_INPUT",
		"blocked":        "ATTENTION_STATE_NEEDS_INPUT",
	} {
		notify := &fakeNotifyClient{}
		n := newAttentionNotifier(&fakeStore{}, notify, 0, fixedNow)

		if got := n.process(context.Background(), goalAttentionEnvelope(t, state, goalStatus, "goal_1")); got != outcomeAck {
			t.Fatalf("%s outcome = %v, want outcomeAck", goalStatus, got)
		}
		if len(notify.accepted) != 1 {
			t.Fatalf("%s notify calls = %d, want 1", goalStatus, len(notify.accepted))
		}
		req := notify.accepted[0]
		if req.Type != "modelplane.goal_stopped" || req.RetentionMode != "standard" {
			t.Fatalf("%s request = %+v, want modelplane.goal_stopped with standard retention", goalStatus, req)
		}
		const wantKey = "attention:org-1:run-abc:user-1:ATTENTION_REASON_GOAL:goal_1"
		if req.IdempotencyKey != wantKey {
			t.Fatalf("%s idempotency key = %q, want %q", goalStatus, req.IdempotencyKey, wantKey)
		}
		if req.Payload["kind"] != "goal_stopped" || req.Payload["message_key"] != "modelplane.goal_stopped" ||
			req.Payload["cta_href"] != "/chat?thread=thread-9" {
			t.Fatalf("%s payload = %+v, want the goal_stopped kind and the thread deep link", goalStatus, req.Payload)
		}
		assertContentFree(t, req.Payload)
		for _, value := range req.Payload {
			if value == goalStatus {
				t.Fatalf("%s payload carries the goal status, which the closed field set has no place for", goalStatus)
			}
		}
	}
}

func TestAttentionNotifierGoalWithAnOpenOrMismatchedStatusNotifiesNothing(t *testing.T) {
	for name, tc := range map[string]struct{ state, goalStatus string }{
		"active goal":           {"ATTENTION_STATE_NEEDS_INPUT", "active"},
		"paused goal":           {"ATTENTION_STATE_NEEDS_INPUT", "paused"},
		"cleared goal":          {"ATTENTION_STATE_NEEDS_INPUT", "cleared"},
		"unknown status":        {"ATTENTION_STATE_NEEDS_INPUT", "judged"},
		"met named as waiting":  {"ATTENTION_STATE_NEEDS_INPUT", "met"},
		"blocked named as done": {"ATTENTION_STATE_COMPLETED", "blocked"},
		"status missing":        {"ATTENTION_STATE_NEEDS_INPUT", ""},
	} {
		notify := &fakeNotifyClient{}
		n := newAttentionNotifier(&fakeStore{}, notify, 0, fixedNow)
		if got := n.process(context.Background(), goalAttentionEnvelope(t, tc.state, tc.goalStatus, "goal_1")); got != outcomeAck {
			t.Fatalf("%s outcome = %v, want outcomeAck", name, got)
		}
		if len(notify.accepted) != 0 {
			t.Fatalf("%s notify calls = %d, want 0", name, len(notify.accepted))
		}
	}
}

func TestAttentionNotifierGoalStopDoesNotFoldIntoAQuestionOnTheSameThread(t *testing.T) {
	notify := &fakeNotifyClient{}
	n := newAttentionNotifier(&fakeStore{}, notify, time.Minute, fixedNow)

	question := attentionEnvelope(t, "ATTENTION_STATE_NEEDS_INPUT", "q-1", "thread-9", envelope.ZDRFlag(false))
	if got := n.process(context.Background(), question); got != outcomeAck {
		t.Fatalf("question outcome = %v, want outcomeAck", got)
	}
	goal := goalAttentionEnvelope(t, "ATTENTION_STATE_NEEDS_INPUT", "turns_reached", "goal_1")
	if got := n.process(context.Background(), goal); got != outcomeAck {
		t.Fatalf("goal outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 2 {
		t.Fatalf("notify calls = %d, want one for the question and one for the goal", len(notify.accepted))
	}
	if notify.accepted[0].Type != "modelplane.needs_input" || notify.accepted[1].Type != "modelplane.goal_stopped" {
		t.Fatalf("types = %q, %q, want needs_input then goal_stopped", notify.accepted[0].Type, notify.accepted[1].Type)
	}
}

func TestAttentionNotifierGoalStoppedIsAckedWhileTheSwitchIsOff(t *testing.T) {
	notify := &fakeNotifyClient{}
	n := &Notifier{store: &fakeStore{}, notify: notify, now: fixedNow}
	data := goalAttentionEnvelope(t, "ATTENTION_STATE_NEEDS_INPUT", "budget_reached", "goal_1")
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 0 {
		t.Fatalf("notify calls = %d, want 0 while the attention switch is off", len(notify.accepted))
	}
}

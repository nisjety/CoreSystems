package runwatch

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/notifyclient"
)

// closedScheduledOutcomeKeys is the only field set a scheduled-outcome notice may carry.
// notification-core's native path refuses any other field, so a field added to the notice
// must join its closed set as well.
var closedScheduledOutcomeKeys = []string{"title", "body", "message_key", "cta_href", "thread_id", "run_id", "outcome"}

// allScheduledOutcomeNames is the closed outcome set, in the order of the contract.
var allScheduledOutcomeNames = []string{"completed", "needs_review", "failed", "paused_expired", "paused_failures"}

var scheduledOutcomeFixedTime = time.Date(2026, 10, 31, 9, 0, 0, 0, time.UTC)

// wellFormedScheduledOutcome is the payload session-core writes for one outcome of run-abc on
// thread-9, owned by subject-7. A test changes one field to show what the consumer refuses.
func wellFormedScheduledOutcome(outcome string) map[string]any {
	return map[string]any{
		"org_id":      "org-1",
		"run_id":      "run-abc",
		"thread_id":   "thread-9",
		"schedule_id": "sched-1",
		"subject_id":  "subject-7",
		"outcome":     outcome,
	}
}

// scheduledOutcomeEnvelope encodes one SCHEDULED_RUN_OUTCOME the way session-core writes it. The
// envelope's user is the run's service owner, so a notice that reaches anyone but the payload's
// subject shows up as a wrong recipient.
func scheduledOutcomeEnvelope(t *testing.T, payload map[string]any, zdr *bool) []byte {
	t.Helper()
	raw, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("encode payload: %v", err)
	}
	env := envelope.Envelope{
		EventID:       "scheduled_run_outcome:session-core:run-abc",
		EventType:     scheduledRunOutcomeEventType,
		SchemaVersion: 1,
		Ts:            scheduledOutcomeFixedTime,
		Producer:      "session-core",
		CorrelationID: "run-abc",
		OrgID:         "org-1",
		UserID:        "service:orchestrator-core",
		ResourceRef:   "run:run-abc",
		Payload:       raw,
		Zdr:           zdr,
	}
	data, err := env.Encode()
	if err != nil {
		t.Fatalf("encode envelope: %v", err)
	}
	return data
}

func newScheduledOutcomeNotifier(store SubscriptionStore, notify NotifyClient) *Notifier {
	n := &Notifier{store: store, notify: notify, now: time.Now}
	WithScheduledOutcomeNotify()(n)
	return n
}

func assertScheduledOutcomeContentFree(t *testing.T, payload map[string]any) {
	t.Helper()
	allowed := make(map[string]bool, len(closedScheduledOutcomeKeys))
	for _, key := range closedScheduledOutcomeKeys {
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
	if strings.Contains(fmt.Sprint(payload), "subject-7") {
		t.Fatal("payload names the subject, which is only the recipient")
	}
}

func TestScheduledOutcomeNotifiesTheSubjectWithOneContentFreeNotice(t *testing.T) {
	store := &fakeStore{}
	notify := &fakeNotifyClient{}
	n := newScheduledOutcomeNotifier(store, notify)

	data := scheduledOutcomeEnvelope(t, wellFormedScheduledOutcome("needs_review"), envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if store.lookups != 0 {
		t.Fatal("a scheduled outcome must not read run watchers")
	}
	if len(notify.accepted) != 1 {
		t.Fatalf("notify calls = %d, want 1", len(notify.accepted))
	}
	req := notify.accepted[0]
	if req.OrganizationID != "org-1" || req.Type != scheduledRunOutcomeNotifyType || req.RetentionMode != retentionModeStandard {
		t.Fatalf("request = %+v, want org-1, %s, standard retention", req, scheduledRunOutcomeNotifyType)
	}
	if req.Recipient.Kind != notifyclient.RecipientKindUser || req.Recipient.ID != "subject-7" {
		t.Fatalf("recipient = %+v, want the payload's subject, never the service owner", req.Recipient)
	}
	const wantKey = "scheduled:org-1:run-abc:needs_review"
	if req.IdempotencyKey != wantKey {
		t.Fatalf("idempotency key = %q, want %q", req.IdempotencyKey, wantKey)
	}
	if req.Payload["message_key"] != "modelplane.scheduled_run_outcome.needs_review" {
		t.Fatalf("message_key = %v, want the outcome-specific key", req.Payload["message_key"])
	}
	if req.Payload["cta_href"] != "/chat?thread=thread-9" || req.Payload["thread_id"] != "thread-9" || req.Payload["run_id"] != "run-abc" {
		t.Fatalf("payload = %+v, want the thread deep link and the run and thread ids", req.Payload)
	}
	assertScheduledOutcomeContentFree(t, req.Payload)
}

func TestScheduledOutcomeIsOffUnlessTheSwitchIsOn(t *testing.T) {
	store := &fakeStore{}
	notify := &fakeNotifyClient{}
	n := &Notifier{store: store, notify: notify, now: time.Now}

	data := scheduledOutcomeEnvelope(t, wellFormedScheduledOutcome("failed"), envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 0 {
		t.Fatal("with the switch off a scheduled outcome must not notify anyone")
	}
}

func TestScheduledOutcomeFailsClosedWithoutADeclaredPersistentPosture(t *testing.T) {
	for _, tc := range []struct {
		name string
		zdr  *bool
	}{
		{name: "undeclared", zdr: nil},
		{name: "zero retention", zdr: envelope.ZDRFlag(true)},
	} {
		t.Run(tc.name, func(t *testing.T) {
			store := &fakeStore{}
			notify := &fakeNotifyClient{}
			n := newScheduledOutcomeNotifier(store, notify)

			data := scheduledOutcomeEnvelope(t, wellFormedScheduledOutcome("completed"), tc.zdr)
			if got := n.process(context.Background(), data); got != outcomeAck {
				t.Fatalf("outcome = %v, want outcomeAck", got)
			}
			if len(notify.accepted) != 0 {
				t.Fatalf("%s posture notified %d times, want none", tc.name, len(notify.accepted))
			}
		})
	}
}

func TestScheduledOutcomeMapsEachClosedOutcomeAndRefusesOthers(t *testing.T) {
	for _, outcome := range allScheduledOutcomeNames {
		store := &fakeStore{}
		notify := &fakeNotifyClient{}
		n := newScheduledOutcomeNotifier(store, notify)

		data := scheduledOutcomeEnvelope(t, wellFormedScheduledOutcome(outcome), envelope.ZDRFlag(false))
		if got := n.process(context.Background(), data); got != outcomeAck {
			t.Fatalf("outcome %s: process = %v, want outcomeAck", outcome, got)
		}
		if len(notify.accepted) != 1 || notify.accepted[0].Payload["outcome"] != outcome {
			t.Fatalf("outcome %s: accepted %d notices, want one carrying the outcome", outcome, len(notify.accepted))
		}
	}

	store := &fakeStore{}
	notify := &fakeNotifyClient{}
	n := newScheduledOutcomeNotifier(store, notify)
	data := scheduledOutcomeEnvelope(t, wellFormedScheduledOutcome("succeeded"), envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("unknown outcome: process = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 0 {
		t.Fatal("an outcome outside the closed set must not notify")
	}
}

func TestScheduledOutcomeRefusesAnEventWithoutSubjectThreadOrMatchingOrg(t *testing.T) {
	for name, edit := range map[string]func(map[string]any){
		"no subject":    func(p map[string]any) { delete(p, "subject_id") },
		"no thread":     func(p map[string]any) { delete(p, "thread_id") },
		"no org":        func(p map[string]any) { delete(p, "org_id") },
		"other org":     func(p map[string]any) { p["org_id"] = "org-2" },
		"blank subject": func(p map[string]any) { p["subject_id"] = "  " },
	} {
		t.Run(name, func(t *testing.T) {
			store := &fakeStore{}
			notify := &fakeNotifyClient{}
			n := newScheduledOutcomeNotifier(store, notify)

			payload := wellFormedScheduledOutcome("needs_review")
			edit(payload)
			data := scheduledOutcomeEnvelope(t, payload, envelope.ZDRFlag(false))
			if got := n.process(context.Background(), data); got != outcomeAck {
				t.Fatalf("outcome = %v, want outcomeAck", got)
			}
			if len(notify.accepted) != 0 {
				t.Fatalf("%s: notified %d times, want none", name, len(notify.accepted))
			}
		})
	}
}

func TestScheduledOutcomeRetriesWhenNotifyCoreRefuses(t *testing.T) {
	store := &fakeStore{}
	notify := &fakeNotifyClient{failFor: map[string]error{"subject-7": errors.New("notification-core unavailable")}}
	n := newScheduledOutcomeNotifier(store, notify)

	data := scheduledOutcomeEnvelope(t, wellFormedScheduledOutcome("needs_review"), envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeRetry {
		t.Fatalf("outcome = %v, want outcomeRetry so JetStream redelivers the notice", got)
	}
	if len(notify.accepted) != 0 {
		t.Fatal("a refused delegation must not count as an accepted notice")
	}
}

// Redelivery sends the same key again; notification-core deduplicates on it, so the consumer
// keeps no delivery state of its own for this path.
func TestScheduledOutcomeRedeliveryReusesTheIdempotencyKey(t *testing.T) {
	store := &fakeStore{}
	notify := &fakeNotifyClient{}
	n := newScheduledOutcomeNotifier(store, notify)

	data := scheduledOutcomeEnvelope(t, wellFormedScheduledOutcome("needs_review"), envelope.ZDRFlag(false))
	for delivery := 1; delivery <= 2; delivery++ {
		if got := n.process(context.Background(), data); got != outcomeAck {
			t.Fatalf("delivery %d: outcome = %v, want outcomeAck", delivery, got)
		}
	}
	if len(notify.accepted) != 2 {
		t.Fatalf("accepted = %d, want both deliveries to reach notification-core", len(notify.accepted))
	}
	if first, second := notify.accepted[0].IdempotencyKey, notify.accepted[1].IdempotencyKey; first != second {
		t.Fatalf("redelivery changed the idempotency key: %q then %q", first, second)
	}
}

func TestScheduledOutcomeNotificationsAreContentFree(t *testing.T) {
	for _, outcome := range allScheduledOutcomeNames {
		store := &fakeStore{}
		notify := &fakeNotifyClient{}
		n := newScheduledOutcomeNotifier(store, notify)

		data := scheduledOutcomeEnvelope(t, wellFormedScheduledOutcome(outcome), envelope.ZDRFlag(false))
		if got := n.process(context.Background(), data); got != outcomeAck {
			t.Fatalf("outcome %s: process = %v, want outcomeAck", outcome, got)
		}
		if len(notify.accepted) != 1 {
			t.Fatalf("outcome %s: notify calls = %d, want 1", outcome, len(notify.accepted))
		}
		assertScheduledOutcomeContentFree(t, notify.accepted[0].Payload)
	}
}

func TestRunCompletedStillNotifiesWatchersWithTheScheduledOutcomeSwitchOn(t *testing.T) {
	store := &fakeStore{watchers: map[string][]Watcher{
		"org-1/run-1": {{ID: "watch-a", UserID: "user-a"}},
	}}
	notify := &fakeNotifyClient{}
	n := newScheduledOutcomeNotifier(store, notify)

	if got := n.process(context.Background(), testEnvelope(t, "RUN_COMPLETED", "org-1", "run-1")); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 1 || notify.accepted[0].Type != "modelplane.run_completed" || notify.accepted[0].Recipient.ID != "user-a" {
		t.Fatalf("accepted = %+v, want one modelplane.run_completed notice to the watcher", notify.accepted)
	}
	if len(store.notified) != 1 || store.notified[0] != "watch-a" {
		t.Fatalf("notified = %v, want the watcher row marked", store.notified)
	}
}

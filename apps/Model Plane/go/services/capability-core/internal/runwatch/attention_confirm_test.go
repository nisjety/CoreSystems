package runwatch

import (
	"bytes"
	"context"
	"errors"
	"log/slog"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/notifyclient"
)

// confirmedNotifier builds a RUN_ATTENTION notifier with the confirmer given, the way the
// service wires it while the attention switch is on.
func confirmedNotifier(store SubscriptionStore, notify NotifyClient, confirm AttentionConfirmer) *Notifier {
	n := &Notifier{store: store, notify: notify, now: fixedNow}
	WithAttentionNotify(0)(n)
	WithAttentionConfirm(confirm)(n)
	return n
}

// An item is confirmed before anything is announced, with the ids its envelope names and
// the state it was raised in.
func TestAttentionAsksSessionCoreBeforeAnnouncing(t *testing.T) {
	confirm := &fakeConfirmer{enabled: true, waiting: true}
	notify := &fakeNotifyClient{}
	n := confirmedNotifier(&fakeStore{}, notify, confirm)

	data := attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	want := confirmQuestion{
		orgID: "org-1", userID: "user-1", threadID: "thread-9", runID: "run-abc",
		sourceID: "apr-1", state: "ATTENTION_STATE_NEEDS_APPROVAL",
	}
	if len(confirm.asked) != 1 || confirm.asked[0] != want {
		t.Fatalf("confirm asked %+v, want exactly %+v", confirm.asked, want)
	}
	if len(notify.accepted) != 1 {
		t.Fatalf("notify calls = %d, want the confirmed item announced once", len(notify.accepted))
	}
}

// An item session-core no longer holds as waiting is acked, and nothing is announced.
func TestAttentionAnItemNoLongerWaitingIsAckedWithoutANotice(t *testing.T) {
	confirm := &fakeConfirmer{enabled: true, waiting: false}
	notify := &fakeNotifyClient{}
	n := confirmedNotifier(&fakeStore{}, notify, confirm)

	data := attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if len(confirm.asked) != 1 || len(notify.accepted) != 0 {
		t.Fatalf("asked %d time(s), notified %d: a decided item must not be announced", len(confirm.asked), len(notify.accepted))
	}
}

// While session-core's attention switch is off the answer is enabled=false. The item is
// acked with no notice, even though it would otherwise still be waiting.
func TestAttentionWhileSessionCoreIsSwitchedOffIsAckedWithoutANotice(t *testing.T) {
	confirm := &fakeConfirmer{enabled: false, waiting: true}
	notify := &fakeNotifyClient{}
	n := confirmedNotifier(&fakeStore{}, notify, confirm)

	data := attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck", got)
	}
	if len(notify.accepted) != 0 {
		t.Fatal("an item session-core has switched off was announced")
	}
}

// A failed ask is retried with backoff and never announced unconfirmed. Once the ask
// answers, the redelivery notifies exactly once.
func TestAttentionAFailedConfirmIsRetriedLaterAndNeverAnnouncedUnconfirmed(t *testing.T) {
	confirm := &fakeConfirmer{enabled: true, waiting: true, err: errors.New("session-core unreachable")}
	notify := &fakeNotifyClient{}
	n := confirmedNotifier(&fakeStore{}, notify, confirm)

	data := attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeRetryLater {
		t.Fatalf("outcome = %v, want outcomeRetryLater when the confirm fails", got)
	}
	if len(notify.accepted) != 0 {
		t.Fatal("an item whose confirm failed was announced")
	}

	confirm.err = nil
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("redelivery outcome = %v, want outcomeAck once the confirm answers", got)
	}
	if len(notify.accepted) != 1 {
		t.Fatalf("notify calls = %d, want exactly one after the redelivery", len(notify.accepted))
	}
}

// Without a confirmer the attention path fails closed: every item is retried and none is
// announced, whatever its state.
func TestAttentionWithoutAConfirmerFailsClosed(t *testing.T) {
	notify := &fakeNotifyClient{}
	n := &Notifier{store: &fakeStore{}, notify: notify, now: fixedNow}
	WithAttentionNotify(0)(n)

	for _, state := range []string{"ATTENTION_STATE_NEEDS_APPROVAL", "ATTENTION_STATE_PLAN_READY", "ATTENTION_STATE_NEEDS_INPUT"} {
		data := attentionEnvelope(t, state, "src-"+state, "thread-9", envelope.ZDRFlag(false))
		if got := n.process(context.Background(), data); got != outcomeRetryLater {
			t.Fatalf("%s outcome = %v, want outcomeRetryLater with no confirmer", state, got)
		}
	}
	if got := n.process(context.Background(), goalAttentionEnvelope(t, "ATTENTION_STATE_COMPLETED", "met", "goal_1")); got != outcomeRetryLater {
		t.Fatalf("goal outcome = %v, want outcomeRetryLater with no confirmer", got)
	}
	if len(notify.accepted) != 0 {
		t.Fatalf("notify calls = %d, want none without a confirmer", len(notify.accepted))
	}
}

// A goal item is confirmed with the state its stop is announced in and its goal id as the
// source, and each confirmed goal is announced.
func TestAttentionAGoalIsConfirmedWithItsStopStateAndGoalID(t *testing.T) {
	confirm := &fakeConfirmer{enabled: true, waiting: true}
	notify := &fakeNotifyClient{}
	n := confirmedNotifier(&fakeStore{}, notify, confirm)

	if got := n.process(context.Background(), goalAttentionEnvelope(t, "ATTENTION_STATE_COMPLETED", "met", "goal_1")); got != outcomeAck {
		t.Fatalf("met goal outcome = %v, want outcomeAck", got)
	}
	if got := n.process(context.Background(), goalAttentionEnvelope(t, "ATTENTION_STATE_NEEDS_INPUT", "budget_reached", "goal_2")); got != outcomeAck {
		t.Fatalf("budget goal outcome = %v, want outcomeAck", got)
	}
	if len(confirm.asked) != 2 {
		t.Fatalf("confirm asked %d time(s), want once per goal", len(confirm.asked))
	}
	if q := confirm.asked[0]; q.state != "ATTENTION_STATE_COMPLETED" || q.sourceID != "goal_1" || q.threadID != "thread-9" {
		t.Fatalf("met goal asked %+v", q)
	}
	if q := confirm.asked[1]; q.state != "ATTENTION_STATE_NEEDS_INPUT" || q.sourceID != "goal_2" {
		t.Fatalf("budget goal asked %+v", q)
	}
	if len(notify.accepted) != 2 {
		t.Fatalf("notify calls = %d, want both confirmed goals announced", len(notify.accepted))
	}
}

// A refusal the same request can never win (a 4xx other than 408 and 429) drops the
// notice: it is acked and not retried.
func TestAttentionAPermanentRefusalIsAckedAndDropped(t *testing.T) {
	confirm := &fakeConfirmer{enabled: true, waiting: true}
	notify := &fakeNotifyClient{failFor: map[string]error{"user-1": &notifyclient.StatusError{Status: http.StatusForbidden}}}
	n := confirmedNotifier(&fakeStore{}, notify, confirm)

	data := attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck for a permanent 403", got)
	}
	if len(notify.accepted) != 0 {
		t.Fatal("a refused notice was counted as accepted")
	}
}

// A 5xx answer is transient: the notice is retried with backoff.
func TestAttentionA502IsRetriedLater(t *testing.T) {
	confirm := &fakeConfirmer{enabled: true, waiting: true}
	notify := &fakeNotifyClient{failFor: map[string]error{"user-1": &notifyclient.StatusError{Status: http.StatusBadGateway}}}
	n := confirmedNotifier(&fakeStore{}, notify, confirm)

	data := attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeRetryLater {
		t.Fatalf("outcome = %v, want outcomeRetryLater for a 502", got)
	}
}

// hungConfirmer is an AttentionConfirmer whose session-core never answers. StillWaiting
// blocks until the context it is given ends, and records whether that context has a deadline.
type hungConfirmer struct {
	deadline bool
}

func (h *hungConfirmer) StillWaiting(ctx context.Context, _, _, _, _, _, _ string) (bool, bool, error) {
	_, h.deadline = ctx.Deadline()
	<-ctx.Done()
	return false, false, ctx.Err()
}

// A confirm that never answers is bounded: the call ends at attentionConfirmTimeout under a
// context with a deadline, the item is retried with backoff, and nothing is announced, so the
// run-watch callback is free for the next event.
func TestAttentionAHungConfirmIsBoundedAndRetriedLater(t *testing.T) {
	saved := attentionConfirmTimeout
	attentionConfirmTimeout = 20 * time.Millisecond
	t.Cleanup(func() { attentionConfirmTimeout = saved })

	confirm := &hungConfirmer{}
	notify := &fakeNotifyClient{}
	n := confirmedNotifier(&fakeStore{}, notify, confirm)

	data := attentionEnvelope(t, "ATTENTION_STATE_NEEDS_APPROVAL", "apr-1", "thread-9", envelope.ZDRFlag(false))
	done := make(chan outcome, 1)
	go func() { done <- n.process(context.Background(), data) }()
	select {
	case got := <-done:
		if got != outcomeRetryLater {
			t.Fatalf("outcome = %v, want outcomeRetryLater for a confirm that timed out", got)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("a confirm that never answered held the notifier past its bound")
	}
	if !confirm.deadline {
		t.Fatal("the confirm ran under a context with no deadline")
	}
	if len(notify.accepted) != 0 {
		t.Fatal("an item whose confirm timed out was announced")
	}
}

// The attention path with no confirmer says so once, at construction. With a confirmer it is silent.
func TestNewNotifierWarnsOnceAtConstructionWhenAttentionHasNoConfirmer(t *testing.T) {
	var logs bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewTextHandler(&logs, nil)))
	t.Cleanup(func() { slog.SetDefault(previous) })

	if _, err := NewNotifier(&fakeStore{}, &fakeNotifyClient{}, WithAttentionNotify(0)); err != nil {
		t.Fatalf("NewNotifier: %v", err)
	}
	if got := strings.Count(logs.String(), "fail closed"); got != 1 {
		t.Fatalf("construction without a confirmer logged %d fail-closed warnings, want 1: %q", got, logs.String())
	}

	logs.Reset()
	if _, err := NewNotifier(&fakeStore{}, &fakeNotifyClient{}, WithAttentionNotify(0), WithAttentionConfirm(&fakeConfirmer{})); err != nil {
		t.Fatalf("NewNotifier: %v", err)
	}
	if logs.Len() != 0 {
		t.Fatalf("construction with a confirmer logged %q, want nothing", logs.String())
	}
}

// A scheduled-run notice refused for good is dropped, as an attention notice is.
func TestScheduledOutcomeAPermanentRefusalIsAckedAndDropped(t *testing.T) {
	store := &fakeStore{}
	notify := &fakeNotifyClient{failFor: map[string]error{"subject-7": &notifyclient.StatusError{Status: http.StatusNotFound}}}
	n := newScheduledOutcomeNotifier(store, notify)

	data := scheduledOutcomeEnvelope(t, wellFormedScheduledOutcome("needs_review"), envelope.ZDRFlag(false))
	if got := n.process(context.Background(), data); got != outcomeAck {
		t.Fatalf("outcome = %v, want outcomeAck for a permanent 404", got)
	}
	if len(notify.accepted) != 0 {
		t.Fatal("a refused scheduled notice was counted as accepted")
	}
}

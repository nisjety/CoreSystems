package runwatch

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/notifyclient"
	"google.golang.org/grpc/status"
)

// runAttentionEventType is the envelope type session-core's sweep writes for a
// thread that waits on its owner (WS16-S3). Its payload names ids and one closed
// state, never text, so a notification built from it carries no run content.
const runAttentionEventType = "RUN_ATTENTION"

// DefaultAttentionCoalesce is the window in which repeated waiting items for one
// user, thread and state fold into the notification already sent.
const DefaultAttentionCoalesce = 60 * time.Second

// attentionCTALabel is the one call to action every attention notification offers.
const attentionCTALabel = "Open chat"

// retentionModeStandard is declared on every attention request. A zero-retention
// run is refused before this point, so the request always names a persistent run.
const retentionModeStandard = "standard"

// attentionKind is what one waiting state notifies: the notification type, the
// short kind the payload names, and the fixed generic copy.
type attentionKind struct {
	notifyType string
	kind       string
	title      string
	body       string
}

// attentionKinds maps each waiting state to its notification. Any other state does
// not wait on the owner, so it is acknowledged without a notification.
var attentionKinds = map[string]attentionKind{
	"ATTENTION_STATE_NEEDS_APPROVAL": {
		notifyType: "modelplane.needs_approval",
		kind:       "needs_approval",
		title:      "Approval needed",
		body:       "A run in one of your chats is waiting for your approval.",
	},
	"ATTENTION_STATE_NEEDS_INPUT": {
		notifyType: "modelplane.needs_input",
		kind:       "needs_input",
		title:      "Input needed",
		body:       "A run in one of your chats has a question for you.",
	},
	"ATTENTION_STATE_PLAN_READY": {
		notifyType: "modelplane.plan_ready",
		kind:       "plan_ready",
		title:      "Plan ready",
		body:       "A plan in one of your chats is ready for your review.",
	},
}

// goalAttentionReason is the reason session-core names on a RUN_ATTENTION for a
// thread goal that stopped (EP-7 S5). An item without it is a pending row.
const goalAttentionReason = "ATTENTION_REASON_GOAL"

// goalStoppedKind is the one notification a stopped goal sends, whatever stopped
// it: fixed copy, no objective and no status text (EP-7 S5).
var goalStoppedKind = attentionKind{
	notifyType: "modelplane.goal_stopped",
	kind:       "goal_stopped",
	title:      "Goal stopped",
	body:       "A goal in one of your chats stopped and may need you.",
}

// goalStopStates maps each goal status that stops a goal to the attention state
// session-core derives for it. A met goal is COMPLETED; a goal stopped by its
// budget, its turn cap or a blocked verdict waits on its owner as NEEDS_INPUT. An
// open goal (active, paused) or a cleared one is not here, so it notifies nothing.
var goalStopStates = map[string]string{
	"met":            "ATTENTION_STATE_COMPLETED",
	"budget_reached": "ATTENTION_STATE_NEEDS_INPUT",
	"turns_reached":  "ATTENTION_STATE_NEEDS_INPUT",
	"blocked":        "ATTENTION_STATE_NEEDS_INPUT",
}

// attentionPayload is the RUN_ATTENTION payload session-core writes. Reason and
// GoalStatus are set only on a goal item (EP-7 S5); a pending-row item omits both.
type attentionPayload struct {
	RunID      string `json:"run_id"`
	SourceID   string `json:"source_id"`
	State      string `json:"state"`
	ThreadID   string `json:"thread_id"`
	Reason     string `json:"reason,omitempty"`
	GoalStatus string `json:"goal_status,omitempty"`
}

// parkedAttentionReason is the reason session-core names on a RUN_ATTENTION for a
// run a crash parked, once the park has waited its notice time (O-02). Such a run
// waits on its owner to continue it, and it is announced only as NEEDS_INPUT.
const parkedAttentionReason = "ATTENTION_REASON_PARKED"

// parkedKind is the notification a crash-parked run sends: fixed copy and a deep
// link, never run content. It is typed with the needs_input notification type that
// notify-core already registers, so no new registration is needed for it.
var parkedKind = attentionKind{
	notifyType: "modelplane.needs_input",
	kind:       "parked",
	title:      "Run waiting for you",
	body:       "A run in one of your chats is waiting for you to continue it.",
}

// pendingRowReasons are the reasons that name a pending row, or no reason at all.
// For these the state alone says what waits, as it always has.
var pendingRowReasons = map[string]bool{
	"":                             true,
	"ATTENTION_REASON_UNSPECIFIED": true,
	"ATTENTION_REASON_APPROVAL":    true,
	"ATTENTION_REASON_PLAN":        true,
	"ATTENTION_REASON_INPUT":       true,
}

// attentionKindOf resolves the notification an attention item sends and the state
// word its coalescing and idempotency keys use. The reason picks the template: a
// pending row takes its state's notification; a goal item resolves only when its
// status stops the goal and agrees with the state session-core derived for it; a
// crash-parked run resolves only as NEEDS_INPUT. A goal and a parked run key on
// their reason, so neither folds into a question on the same thread. A reason this
// build does not name resolves to nothing, so an unknown reason is never notified.
func attentionKindOf(payload attentionPayload) (attentionKind, string, bool) {
	reason := strings.TrimSpace(payload.Reason)
	switch {
	case pendingRowReasons[reason]:
		kind, ok := attentionKinds[payload.State]
		return kind, payload.State, ok
	case reason == goalAttentionReason:
		state, ok := goalStopStates[strings.TrimSpace(payload.GoalStatus)]
		if !ok || state != payload.State {
			return attentionKind{}, "", false
		}
		return goalStoppedKind, goalAttentionReason, true
	case reason == parkedAttentionReason:
		if payload.State != "ATTENTION_STATE_NEEDS_INPUT" {
			return attentionKind{}, "", false
		}
		return parkedKind, parkedAttentionReason, true
	default:
		return attentionKind{}, "", false
	}
}

// NotifierOption configures a Notifier.
type NotifierOption func(*Notifier)

// WithAttentionNotify turns on the RUN_ATTENTION path. Without it a RUN_ATTENTION
// is acknowledged like any other non-terminal event, so the switch being off
// changes nothing for existing traffic. With it, WithAttentionConfirm is required:
// a Notifier built with no confirmer fails closed and announces nothing.
func WithAttentionNotify(coalesce time.Duration) NotifierOption {
	return func(n *Notifier) {
		n.attention = &attentionNotify{
			coalesce: coalesce,
			lastSent: make(map[coalesceKey]time.Time),
		}
	}
}

// AttentionConfirmer asks session-core whether one waiting item still waits on its
// owner before the item is announced (F-f05-03), so an item decided since its
// event was written is not announced again. enabled is false while session-core's
// attention switch is off, and waiting is false once the item is decided. The
// answer carries no content.
type AttentionConfirmer interface {
	StillWaiting(ctx context.Context, orgID, userID, threadID, runID, sourceID, state string) (enabled bool, waiting bool, err error)
}

// WithAttentionConfirm sets the confirmer the RUN_ATTENTION path asks before it
// announces an item. It is mandatory while WithAttentionNotify is on.
func WithAttentionConfirm(c AttentionConfirmer) NotifierOption {
	return func(n *Notifier) {
		n.attentionConfirm = c
	}
}

// attentionConfirmTimeout bounds one StillWaiting call. process runs on the JetStream
// callback of the run-watch subscription, one message at a time, so a call that never
// answers would hold every later run-watch event, terminal-run notices included, and
// its message would be redelivered on the durable's 30s AckWait, spending one of its
// five deliveries. Ten seconds leaves room for a slow peer and stays under that AckWait.
// A timeout is a transient failure: the item is retried with backoff, never announced.
// It is a var so a test can shorten it.
var attentionConfirmTimeout = 10 * time.Second

// ParseAttentionCoalesce reads ATTENTION_NOTIFY_COALESCE_SECONDS. A value that is
// not a non-negative whole number of seconds reads as the default, so a typo
// cannot silently turn coalescing off or stretch it without bound.
func ParseAttentionCoalesce(value string) time.Duration {
	seconds, err := strconv.Atoi(strings.TrimSpace(value))
	if err != nil || seconds < 0 {
		return DefaultAttentionCoalesce
	}
	return time.Duration(seconds) * time.Second
}

// attentionNotify is the RUN_ATTENTION half of a Notifier. Its coalescing map lives
// in this process and is best effort: a second replica may notify once more in the
// same window. The idempotency key cannot close that gap, because it names the item.
type attentionNotify struct {
	coalesce time.Duration

	mu       sync.Mutex
	lastSent map[coalesceKey]time.Time
}

// coalesceKey is one user's view of one thread waiting in one state. A burst of
// approvals on that thread reads as one notification.
type coalesceKey struct {
	orgID    string
	userID   string
	threadID string
	state    string
}

// coalesced reports whether key was notified within the window before now.
func (a *attentionNotify) coalesced(key coalesceKey, now time.Time) bool {
	if a.coalesce <= 0 {
		return false
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	last, ok := a.lastSent[key]
	return ok && now.Sub(last) < a.coalesce
}

// markSent records a successful notification for key. It also forgets every key
// whose window has closed, so the map holds only the streams of the last window.
func (a *attentionNotify) markSent(key coalesceKey, now time.Time) {
	if a.coalesce <= 0 {
		return
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	for stale, last := range a.lastSent {
		if now.Sub(last) >= a.coalesce {
			delete(a.lastSent, stale)
		}
	}
	a.lastSent[key] = now
}

// processAttention notifies the user a RUN_ATTENTION names, at most once per
// coalescing window, and only once session-core confirms the item still waits. It
// fails closed: with no confirmer every event is retried and none is announced, and
// the envelope must declare the run persistent (zdr false), so an omitted flag is
// refused exactly as a true one is.
func (n *Notifier) processAttention(ctx context.Context, env *envelope.Envelope) outcome {
	if n.attentionConfirm == nil {
		// Fail closed: with no confirmer no waiting item may be announced. The event is
		// retried with backoff, and JetStream drops it at MaxDeliver, rather than acked as
		// if it had been handled.
		return outcomeRetryLater
	}
	if env.Zdr == nil || *env.Zdr {
		slog.Warn("runwatch: attention event refused: run did not declare a persistent posture")
		return outcomeAck
	}
	var payload attentionPayload
	if err := json.Unmarshal(env.Payload, &payload); err != nil {
		slog.Warn("runwatch: decode attention payload failed", "error", err)
		return outcomeAck
	}
	kind, stateKey, ok := attentionKindOf(payload)
	if !ok {
		return outcomeAck
	}
	orgID := strings.TrimSpace(env.OrgID)
	userID := strings.TrimSpace(env.UserID)
	runID := runIDFrom(env)
	threadID := strings.TrimSpace(payload.ThreadID)
	sourceID := strings.TrimSpace(payload.SourceID)
	if orgID == "" || userID == "" || runID == "" || threadID == "" || sourceID == "" {
		slog.Warn("runwatch: attention event missing org, user, run, thread or source id", "state", stateKey)
		return outcomeAck
	}

	// Ask before anything is announced (F-f05-03). The state is the attention state the item
	// was raised in: the row's own state, which for a goal is COMPLETED or NEEDS_INPUT. An item
	// session-core no longer holds as waiting, or one raised while its switch is off, is acked
	// without a notice; a failed or timed-out ask is retried, never announced unconfirmed. The
	// ask is bounded by attentionConfirmTimeout, so a peer that never answers cannot hold the
	// run-watch callback past the durable's AckWait.
	cctx, cancel := context.WithTimeout(ctx, attentionConfirmTimeout)
	enabled, waiting, err := n.attentionConfirm.StillWaiting(cctx, orgID, userID, threadID, runID, sourceID, payload.State)
	cancel()
	if err != nil {
		slog.Warn("runwatch: attention confirm failed", "org_id", orgID, "run_id", runID, "code", status.Code(err).String())
		return outcomeRetryLater
	}
	if !enabled || !waiting {
		return outcomeAck
	}

	now := n.now().UTC()
	key := coalesceKey{orgID: orgID, userID: userID, threadID: threadID, state: stateKey}
	if n.attention.coalesced(key, now) {
		return outcomeAck
	}
	req := notifyclient.Request{
		OrganizationID: orgID,
		IdempotencyKey: attentionIdempotencyKey(orgID, runID, userID, stateKey, sourceID),
		Recipient:      notifyclient.Recipient{Kind: notifyclient.RecipientKindUser, ID: userID},
		Type:           kind.notifyType,
		Payload:        attentionNotificationPayload(kind, threadID, runID),
		RetentionMode:  retentionModeStandard,
	}
	if err := n.notify.Accept(ctx, req); err != nil {
		if notifyclient.IsPermanent(err) {
			// A refusal this request can never win: the notice is dropped, not retried.
			statusCode, _ := notifyclient.StatusOf(err)
			slog.Warn("runwatch: attention notice refused by notify-core, dropped", "org_id", orgID, "run_id", runID, "status", statusCode)
			return outcomeAck
		}
		// Not marked as sent, so the redelivery of this event notifies after all.
		slog.Warn("runwatch: attention notify-core delegation failed", "org_id", orgID, "run_id", runID, "user_id", userID, "error", err)
		return outcomeRetryLater
	}
	n.attention.markSent(key, now)
	return outcomeAck
}

// attentionIdempotencyKey names one waiting item. session-core derives its event id
// from the same run, state and source, so a redelivered event and a re-sweep of the
// same row both resolve to one notification.
func attentionIdempotencyKey(orgID, runID, userID, state, sourceID string) string {
	return fmt.Sprintf("attention:%s:%s:%s:%s:%s", orgID, runID, userID, state, sourceID)
}

// attentionNotificationPayload is the notification body: fixed copy, a deep link
// into the thread and opaque ids. Nothing the run produced is in it.
func attentionNotificationPayload(kind attentionKind, threadID, runID string) map[string]any {
	return map[string]any{
		"title":       kind.title,
		"body":        kind.body,
		"message_key": kind.notifyType,
		"cta_href":    "/chat?thread=" + url.QueryEscape(threadID),
		"cta_label":   attentionCTALabel,
		"thread_id":   threadID,
		"run_id":      runID,
		"kind":        kind.kind,
	}
}

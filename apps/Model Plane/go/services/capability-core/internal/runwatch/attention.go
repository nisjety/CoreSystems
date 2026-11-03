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

// attentionKindOf resolves the notification an attention item sends and the
// state word its coalescing and idempotency keys use. A goal item resolves only
// when its status stops the goal and agrees with the state session-core derived
// for it; its key word is the goal reason, so a goal stop never folds into a
// question on the same thread.
func attentionKindOf(payload attentionPayload) (attentionKind, string, bool) {
	if strings.TrimSpace(payload.Reason) != goalAttentionReason {
		kind, ok := attentionKinds[payload.State]
		return kind, payload.State, ok
	}
	state, ok := goalStopStates[strings.TrimSpace(payload.GoalStatus)]
	if !ok || state != payload.State {
		return attentionKind{}, "", false
	}
	return goalStoppedKind, goalAttentionReason, true
}

// NotifierOption configures a Notifier.
type NotifierOption func(*Notifier)

// WithAttentionNotify turns on the RUN_ATTENTION path. Without it a RUN_ATTENTION
// is acknowledged like any other non-terminal event, so the switch being off
// changes nothing for existing traffic.
func WithAttentionNotify(coalesce time.Duration) NotifierOption {
	return func(n *Notifier) {
		n.attention = &attentionNotify{
			coalesce: coalesce,
			lastSent: make(map[coalesceKey]time.Time),
		}
	}
}

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
// coalescing window. It fails closed: the envelope must declare the run persistent
// (zdr false), so an omitted flag is refused exactly as a true one is.
func (n *Notifier) processAttention(ctx context.Context, env *envelope.Envelope) outcome {
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
		// Not marked as sent, so the redelivery of this event notifies after all.
		slog.Warn("runwatch: attention notify-core delegation failed", "org_id", orgID, "run_id", runID, "user_id", userID, "error", err)
		return outcomeRetry
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

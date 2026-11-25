package runwatch

import (
	"context"
	"encoding/json"
	"fmt"
	"log/slog"
	"net/url"
	"strings"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/notifyclient"
)

// scheduledRunOutcomeEventType is the envelope type session-core writes when a
// scheduled run terminalizes (WS20-S7). Its payload names ids and one closed
// outcome, never text, so a notice built from it carries no run content.
const scheduledRunOutcomeEventType = "SCHEDULED_RUN_OUTCOME"

// scheduledRunOutcomeNotifyType is the notification type the outcome notice is
// sent as. notification-core's isNotificationTypeAuthorized allowlists it for
// capability-core.
const scheduledRunOutcomeNotifyType = "modelplane.scheduled_run_outcome"

// scheduledOutcomeCopy is the fixed copy one outcome's notice shows.
type scheduledOutcomeCopy struct {
	title string
	body  string
}

// scheduledOutcomes is the closed set of outcomes a notice covers. An outcome
// outside it is acknowledged without a notice, so a new outcome cannot reach a
// user before its copy exists.
var scheduledOutcomes = map[string]scheduledOutcomeCopy{
	"completed": {
		title: "Scheduled run finished",
		body:  "A scheduled run in one of your chats finished.",
	},
	"needs_review": {
		title: "Scheduled run needs review",
		body:  "A scheduled run in one of your chats needs your review.",
	},
	"failed": {
		title: "Scheduled run failed",
		body:  "A scheduled run in one of your chats did not finish.",
	},
	"paused_expired": {
		title: "Schedule paused",
		body:  "A schedule in one of your chats paused because its approval expired.",
	},
	"paused_failures": {
		title: "Schedule paused",
		body:  "A schedule in one of your chats paused after repeated failures.",
	},
}

// scheduledOutcomePayload is the SCHEDULED_RUN_OUTCOME payload session-core writes. The run id is
// read by runIDFrom, which every run event shares. ScheduleID names the schedule the run fired from,
// and SubjectID must be that schedule's creator before anything is sent (see ScheduleOwnerChecker).
type scheduledOutcomePayload struct {
	OrgID      string `json:"org_id"`
	ThreadID   string `json:"thread_id"`
	ScheduleID string `json:"schedule_id"`
	SubjectID  string `json:"subject_id"`
	Outcome    string `json:"outcome"`
}

// WithScheduledOutcomeNotify turns on the SCHEDULED_RUN_OUTCOME path. Without it
// such an event is acknowledged like any other non-terminal event, so the switch
// being off changes nothing for existing traffic.
func WithScheduledOutcomeNotify() NotifierOption {
	return func(n *Notifier) {
		n.scheduledOutcome = true
	}
}

// ScheduleOwnerChecker answers whether a schedule is live in an org and was created by the subject a
// SCHEDULED_RUN_OUTCOME names. The envelope's producer is not authenticated, so the payload's subject is
// trusted only on this answer, which capability-core reads from its own cron_schedules table (F-f05-06).
// A false answer covers a schedule of another org, a schedule created by someone else and a deleted one.
type ScheduleOwnerChecker interface {
	ScheduleCreatedBy(ctx context.Context, orgID, scheduleID, subjectID string) (bool, error)
}

// WithScheduleOwnerCheck sets the checker a scheduled-outcome notice must pass before it is sent. It is
// mandatory while WithScheduledOutcomeNotify is on: a Notifier built without one acknowledges every
// scheduled outcome and announces none.
func WithScheduleOwnerCheck(c ScheduleOwnerChecker) NotifierOption {
	return func(n *Notifier) {
		n.scheduleOwner = c
	}
}

// processScheduledOutcome sends the subject of a scheduled run one notice for the
// run's terminal outcome. It fails closed: the envelope must declare the run
// persistent (zdr false), so an omitted flag is refused exactly as a true one is.
// The notice goes to the subject the payload names only once capability-core's own
// schedule table confirms that subject created the schedule the payload names (F-f05-06):
// the envelope's producer is not authenticated, so the payload cannot vouch for itself.
// The envelope's user is the run's service owner and is never the recipient.
func (n *Notifier) processScheduledOutcome(ctx context.Context, env *envelope.Envelope) outcome {
	if env.Zdr == nil || *env.Zdr {
		slog.Warn("runwatch: scheduled outcome refused: run did not declare a persistent posture")
		return outcomeAck
	}
	var payload scheduledOutcomePayload
	if err := json.Unmarshal(env.Payload, &payload); err != nil {
		slog.Warn("runwatch: decode scheduled outcome payload failed", "error", err)
		return outcomeAck
	}
	text, ok := scheduledOutcomes[payload.Outcome]
	if !ok {
		return outcomeAck
	}
	orgID := strings.TrimSpace(env.OrgID)
	runID := runIDFrom(env)
	threadID := strings.TrimSpace(payload.ThreadID)
	subjectID := strings.TrimSpace(payload.SubjectID)
	if orgID == "" || runID == "" || threadID == "" || subjectID == "" || strings.TrimSpace(payload.OrgID) != orgID {
		slog.Warn("runwatch: scheduled outcome missing or mismatched org, run, thread or subject id", "outcome", payload.Outcome)
		return outcomeAck
	}
	// Ask capability-core's own schedule table who created the schedule before anything is announced
	// (F-f05-06). The ask is bounded like the attention confirmation, so a stalled database cannot hold
	// the run-watch callback past the durable's AckWait.
	if n.scheduleOwner == nil {
		// Fail closed: with no owner check no scheduled outcome may be announced. The event is
		// acknowledged, so it is dropped rather than retried for a check that is not wired.
		slog.Warn("runwatch: scheduled outcome not announced: no schedule owner check is wired", "org_id", orgID, "run_id", runID)
		return outcomeAck
	}
	scheduleID := strings.TrimSpace(payload.ScheduleID)
	if scheduleID == "" {
		slog.Warn("runwatch: scheduled outcome not announced: the payload names no schedule", "org_id", orgID, "run_id", runID)
		return outcomeAck
	}
	cctx, cancel := context.WithTimeout(ctx, attentionConfirmTimeout)
	owner, err := n.scheduleOwner.ScheduleCreatedBy(cctx, orgID, scheduleID, subjectID)
	cancel()
	if err != nil {
		// Not acknowledged: the redelivery asks again, and nothing is announced unconfirmed.
		slog.Warn("runwatch: scheduled outcome owner check failed", "org_id", orgID, "run_id", runID, "error", err)
		return outcomeRetryLater
	}
	if !owner {
		// Ids only: neither the subject id nor the schedule's creator is logged.
		slog.Warn("runwatch: scheduled outcome subject does not match the schedule creator", "org_id", orgID, "run_id", runID, "schedule_id", scheduleID)
		return outcomeAck
	}

	req := notifyclient.Request{
		OrganizationID: orgID,
		IdempotencyKey: scheduledOutcomeIdempotencyKey(orgID, runID, payload.Outcome),
		Recipient:      notifyclient.Recipient{Kind: notifyclient.RecipientKindUser, ID: subjectID},
		Type:           scheduledRunOutcomeNotifyType,
		Payload:        scheduledOutcomeNotificationPayload(text, payload.Outcome, threadID, runID),
		RetentionMode:  retentionModeStandard,
	}
	if err := n.notify.Accept(ctx, req); err != nil {
		if notifyclient.IsPermanent(err) {
			// A refusal this request can never win: the notice is dropped, not retried.
			statusCode, _ := notifyclient.StatusOf(err)
			slog.Warn("runwatch: scheduled outcome refused by notify-core, dropped", "org_id", orgID, "run_id", runID, "status", statusCode)
			return outcomeAck
		}
		// Not acknowledged, so the redelivery of this event notifies after all.
		slog.Warn("runwatch: scheduled outcome notify-core delegation failed", "org_id", orgID, "run_id", runID, "error", err)
		return outcomeRetryLater
	}
	return outcomeAck
}

// scheduledOutcomeIdempotencyKey names one run's outcome. A redelivered event for
// the same run and outcome resolves to the notice already accepted.
func scheduledOutcomeIdempotencyKey(orgID, runID, outcome string) string {
	return fmt.Sprintf("scheduled:%s:%s:%s", orgID, runID, outcome)
}

// scheduledOutcomeNotificationPayload is the notice body: fixed copy, a deep link
// into the thread, the closed outcome token and opaque ids. Nothing the run
// produced is in it.
func scheduledOutcomeNotificationPayload(text scheduledOutcomeCopy, outcome, threadID, runID string) map[string]any {
	return map[string]any{
		"title":       text.title,
		"body":        text.body,
		"message_key": scheduledRunOutcomeNotifyType + "." + outcome,
		"cta_href":    "/chat?thread=" + url.QueryEscape(threadID),
		"thread_id":   threadID,
		"run_id":      runID,
		"outcome":     outcome,
	}
}

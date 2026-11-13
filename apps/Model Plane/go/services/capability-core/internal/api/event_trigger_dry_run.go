package api

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/eventtrigger"
)

// The trigger dry run (EP-7, F-t3-047, slice W4-F T6) answers one question about one stored webhook event: would a fire of
// this trigger wake, and why. The caller must be the trigger's member, in the trigger's org. The event must have come from the
// trigger's own connection (the event-to-connection binding of W4-T6). An event from another connection, or one with no
// connection recorded, answers as a missing event trigger does, so the answer confirms nothing about that event. The answer
// is the wake gate's own decision (eventtrigger.DryRunWakeGate) and carries no payload, no field value and no record of the
// event. Nothing is written, no fire is recorded and Control is not asked. The route is mounted only while
// EVENT_TRIGGER_WAKE_GATE_ENABLED is the literal true (cmd/main.go); otherwise the path answers 404.

// eventTriggerRoutePrefix is the path the dry run is mounted under. The full path is this, the trigger id, then the tail.
const eventTriggerRoutePrefix = "/api/v1/event-triggers/"

// eventTriggerDryRunTail ends the dry run's path.
const eventTriggerDryRunTail = "/dry-run"

// eventTriggerDryRunBodyBytes bounds the request body, which holds one webhook event id.
const eventTriggerDryRunBodyBytes = 4 << 10

// eventTriggerIDChars bounds a trigger id in the path and a webhook event id in the body.
const eventTriggerIDChars = 128

// eventTriggerDryRunPerMinute is how many dry runs one trigger may make in a rolling minute, per process.
const eventTriggerDryRunPerMinute = 10

// eventTriggerDryRunWindow is the span the per-trigger limit counts over.
const eventTriggerDryRunWindow = time.Minute

// eventTriggerDryRunTracked is how many triggers the limiter keeps counts for before it drops the counts that have aged out.
const eventTriggerDryRunTracked = 4096

// eventTriggerDryRunZDRCode is the typed refusal for a dry run under zero data retention or without a stated posture.
const eventTriggerDryRunZDRCode = "event_trigger_dry_run_under_zdr_refused"

// eventTriggerDB is the one read the dry run makes, the trigger row. *pgxpool.Pool satisfies it.
type eventTriggerDB interface {
	QueryRow(context.Context, string, ...any) pgx.Row
}

// EventTriggerDryRunHandler serves POST /api/v1/event-triggers/{trigger_id}/dry-run.
type EventTriggerDryRunHandler struct {
	db      eventTriggerDB
	source  eventtrigger.WebhookEventSource
	limiter *dryRunRateLimiter
	now     func() time.Time
}

// NewEventTriggerDryRunHandler builds the dry run over the trigger table and the webhook event source the wake gate reads.
func NewEventTriggerDryRunHandler(db eventTriggerDB, source eventtrigger.WebhookEventSource) *EventTriggerDryRunHandler {
	return &EventTriggerDryRunHandler{db: db, source: source, limiter: newDryRunRateLimiter(), now: time.Now}
}

// Register mounts the dry run on mux. The caller mounts it only while EVENT_TRIGGER_WAKE_GATE_ENABLED is the literal true, so
// otherwise no route exists and the path answers 404.
func (h *EventTriggerDryRunHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc(eventTriggerRoutePrefix, h.route)
}

// route answers the one path and method the dry run serves. Any other path under the prefix is not found, as the cron routes
// answer theirs.
func (h *EventTriggerDryRunHandler) route(w http.ResponseWriter, r *http.Request) {
	triggerID, ok := strings.CutSuffix(strings.TrimPrefix(r.URL.Path, eventTriggerRoutePrefix), eventTriggerDryRunTail)
	if !ok || r.Method != http.MethodPost || triggerID == "" || len(triggerID) > eventTriggerIDChars || strings.Contains(triggerID, "/") {
		http.NotFound(w, r)
		return
	}
	h.dryRun(w, r, triggerID)
}

// eventTriggerDryRunData is the body of a dry run that was made: the verdict, its reason and the names of the rules it met.
type eventTriggerDryRunData struct {
	WouldWake bool     `json:"would_wake"`
	Reason    string   `json:"reason"`
	Matched   []string `json:"matched"`
}

// dryRun makes one dry run for the verified member of the trigger. The refusals come in this order, and each one is decided
// before any payload is read: the caller's identity and posture, the body, the member, then the per-trigger limit. The event is
// read after them, and it gets a verdict only when it belongs to the trigger's connection.
func (h *EventTriggerDryRunHandler) dryRun(w http.ResponseWriter, r *http.Request, triggerID string) {
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		writeTypedError(w, http.StatusForbidden, "not_authorized", "the request is not authorized")
		return
	}
	// The router refuses every non-read request from a zero-retention principal, or one with no stated posture, before this
	// handler runs. The check is repeated here so that no payload is ever read for such a caller.
	if principal.ZeroDataRetention || !principal.RetentionPolicyPresent {
		writeTypedError(w, http.StatusForbidden, eventTriggerDryRunZDRCode,
			"event trigger dry runs are not available under zero data retention or without a stated retention posture")
		return
	}
	// A trigger belongs to one person. A service or an api key owns none, so it is refused before anything is read.
	if principal.PrincipalType != "user" {
		writeTypedError(w, http.StatusForbidden, "event_trigger_member_only", "an event trigger belongs to its member")
		return
	}
	var body struct {
		WebhookEventID string `json:"webhook_event_id"`
	}
	decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, eventTriggerDryRunBodyBytes))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&body); err != nil {
		writeTypedError(w, http.StatusBadRequest, "invalid_request", "the body must be a JSON object with a webhook_event_id")
		return
	}
	if _, err := decoder.Token(); !errors.Is(err, io.EOF) {
		writeTypedError(w, http.StatusBadRequest, "invalid_request", "the body must be a single JSON object")
		return
	}
	webhookEventID := strings.TrimSpace(body.WebhookEventID)
	if webhookEventID == "" || utf8.RuneCountInString(webhookEventID) > eventTriggerIDChars {
		writeTypedError(w, http.StatusBadRequest, "invalid_request", "webhook_event_id is required and is at most 128 characters")
		return
	}
	var memberID, connectionID string
	var wakeFilter json.RawMessage
	err := h.db.QueryRow(r.Context(), `
		SELECT member_user_id, connection_id, wake_filter
		FROM event_triggers
		WHERE id = $1 AND org_id = $2`, triggerID, principal.OrganizationID).Scan(&memberID, &connectionID, &wakeFilter)
	if errors.Is(err, pgx.ErrNoRows) {
		writeTypedError(w, http.StatusNotFound, "event_trigger_not_found", "the event trigger was not found")
		return
	}
	if err != nil {
		slog.Warn("event trigger dry run could not read its trigger", "trigger_id", triggerID, "error", err)
		writeTypedError(w, http.StatusInternalServerError, "event_trigger_dry_run_unavailable", "the dry run could not be made")
		return
	}
	// Another member's trigger answers as a missing one, so the id confirms no one else's record.
	if memberID != principal.ActorID {
		writeTypedError(w, http.StatusNotFound, "event_trigger_not_found", "the event trigger was not found")
		return
	}
	if !h.limiter.allow(triggerID, h.now()) {
		writeTypedError(w, http.StatusTooManyRequests, "event_trigger_dry_run_rate_limited",
			"this trigger has been dry run too often in the last minute")
		return
	}
	result, err := eventtrigger.DryRunWakeGate(r.Context(), h.source, principal.OrganizationID, connectionID, webhookEventID, wakeFilter)
	// An event of another connection, or one with no connection recorded, answers as a missing trigger does, so the answer
	// neither confirms the event nor names its connection.
	if errors.Is(err, eventtrigger.ErrDryRunEventNotOfTrigger) {
		writeTypedError(w, http.StatusNotFound, "event_trigger_not_found", "the event trigger was not found")
		return
	}
	if err != nil {
		slog.Warn("event trigger dry run could not read the webhook event", "trigger_id", triggerID, "error", err)
		writeTypedError(w, http.StatusServiceUnavailable, "wake_payload_unavailable", "the webhook event could not be read now")
		return
	}
	writeJSON(w, map[string]any{"data": eventTriggerDryRunData{
		WouldWake: result.WouldWake,
		Reason:    result.Reason,
		Matched:   result.Matched,
	}})
}

// dryRunRateLimiter counts the dry runs of each trigger over the last minute, in this process's memory. A refused dry run is
// not counted, so a caller who keeps asking waits for the window to pass.
type dryRunRateLimiter struct {
	mu    sync.Mutex
	calls map[string][]time.Time
}

// newDryRunRateLimiter builds an empty limiter.
func newDryRunRateLimiter() *dryRunRateLimiter {
	return &dryRunRateLimiter{calls: map[string][]time.Time{}}
}

// allow records one dry run of triggerID at now and reports whether it is within the limit.
func (l *dryRunRateLimiter) allow(triggerID string, now time.Time) bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	cutoff := now.Add(-eventTriggerDryRunWindow)
	if len(l.calls) >= eventTriggerDryRunTracked {
		for id, times := range l.calls {
			if kept := dryRunCallsAfter(times, cutoff); len(kept) == 0 {
				delete(l.calls, id)
			} else {
				l.calls[id] = kept
			}
		}
	}
	recent := dryRunCallsAfter(l.calls[triggerID], cutoff)
	if len(recent) >= eventTriggerDryRunPerMinute {
		l.calls[triggerID] = recent
		return false
	}
	l.calls[triggerID] = append(recent, now)
	return true
}

// dryRunCallsAfter returns the calls in times, which are in the order they were made, that fall after cutoff.
func dryRunCallsAfter(times []time.Time, cutoff time.Time) []time.Time {
	i := 0
	for i < len(times) && !times[i].After(cutoff) {
		i++
	}
	return times[i:]
}

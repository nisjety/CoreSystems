// Package runwatch is the AUTO-2 ("notify me when a run finishes") trigger:
// it turns a terminal run-lifecycle event into zero or more delegated
// notification-core calls, one per user who registered a watch on that run
// (internal/api's RunWatchersHandler is where a watch gets registered).
//
// # Design
//
// Event-driven via a JetStream DURABLE consumer with manual ack — unlike
// taskexec.RunCompletionConsumer and sessionreview.RunConsumer, which both
// use a plain core-NATS nc.Subscribe/QueueSubscribe (fire-and-forget,
// at-most-once from JetStream's perspective). Those two consumers can afford
// that because their side effects are cheap local reads/writes; this
// consumer's side effect is an OUTBOUND CROSS-PLANE HTTP CALL to
// notification-core, which can fail transiently (network blip,
// notification-core briefly down, etc.), and a lost RUN_COMPLETED here means
// a user silently never finds out their run finished. Manual ack lets a
// notify failure Nak the message so JetStream redelivers it — safe because
// [Notifier.process] only ever flips a row from 'pending' to 'notified' once
// the notify call succeeds, so redelivery re-derives the same still-pending
// watcher set and simply retries the ones that failed.
//
// Every dependency below process is a small interface ([SubscriptionStore],
// [notifyclient.Client] via the narrower [NotifyClient]), so the decode ->
// match -> lookup -> notify -> persist decision logic is fully unit-tested
// with fakes; only [Notifier.Run] (the actual JetStream bind) needs a live
// broker.
package runwatch

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"github.com/nats-io/nats.go"

	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/notifyclient"
)

// RunEventSubject is the run-lifecycle subject filter this consumer binds to.
// RUN_COMPLETED and RUN_FAILED envelopes for every run in the plane arrive
// here; only ones with at least one pending watcher cause any further work.
// (This is the same literal subject as taskexec.RunEventSubject and
// sessionreview.RunCompletedSubject — each consumer of this subject defines
// its own copy, matching the existing convention in this package tree rather
// than introducing a shared cross-package constant.)
const RunEventSubject = "mp.v1.run.*.event"

// RunEventsStream is the JetStream stream this consumer's durable is bound
// to. Must match nats-provisioner's stream name exactly (services/
// nats-provisioner/main.go's streamConfigs, "MODEL_PLANE_RUN_EVENTS").
const RunEventsStream = "MODEL_PLANE_RUN_EVENTS"

// RunWatchDurable is this consumer's durable name. Must exactly match the
// Durable value nats-provisioner registers server-side for this stream (see
// its consumerBindings) — binding is Bind-only (never self-provisioning), so
// a name mismatch here fails Run's subscribe outright rather than silently
// missing events.
const RunWatchDurable = "capability-core-run-watch-notify"

// Notification types this consumer sends, matching notification-core's
// isNotificationTypeAuthorized allowlist entry for principal "capability-core".
const (
	notifyTypeRunCompleted = "modelplane.run_completed"
	notifyTypeRunFailed    = "modelplane.run_failed"
)

// terminalNotificationType maps a run-lifecycle event to the notification
// type it fires, mirroring taskexec.terminalStatus's event-type check.
// Anything else (RUN_STARTED, step events) is not terminal and is ignored.
func terminalNotificationType(eventType string) (notifyType string, ok bool) {
	switch eventType {
	case "RUN_COMPLETED":
		return notifyTypeRunCompleted, true
	case "RUN_FAILED":
		return notifyTypeRunFailed, true
	default:
		return "", false
	}
}

// Watcher is one pending run-watch row: who to notify.
type Watcher struct {
	ID     string
	UserID string
}

// SubscriptionStore is the narrow run_watch_subscriptions surface this
// consumer needs. PostgresSubscriptionStore (below) is the real
// implementation; tests inject a fake.
type SubscriptionStore interface {
	// PendingWatchers returns every still-pending, non-deleted watcher for
	// (orgID, runID). Empty (nil, nil) is the common case: most runs have no
	// watchers at all.
	PendingWatchers(ctx context.Context, orgID, runID string) ([]Watcher, error)
	// MarkNotified flips one watcher row from 'pending' to 'notified'. It
	// must be guarded on status='pending' so a redelivered event that
	// already notified this row on a prior delivery is a no-op, not a
	// duplicate notification.
	MarkNotified(ctx context.Context, watcherID, eventType string, notifiedAt time.Time) error
}

// NotifyClient is the narrow notifyclient surface this consumer needs —
// satisfied by *notifyclient.Client, faked in tests.
type NotifyClient interface {
	Accept(ctx context.Context, req notifyclient.Request) error
}

// outcome tells the JetStream handler whether to ack (done, or a definitive
// no-op skip) or Nak (a transient failure — redeliver).
type outcome int

const (
	outcomeAck outcome = iota
	outcomeRetry
)

// Notifier subscribes to run-lifecycle events and, for each terminal event,
// notifies every pending watcher of that run.
type Notifier struct {
	store  SubscriptionStore
	notify NotifyClient
	now    func() time.Time
}

// NewNotifier constructs a Notifier. Both dependencies are required: without
// a store there is nothing to look up, and without a notify client there is
// nothing to call, so callers should skip constructing this at all (logging
// why) rather than run a Notifier that can only ever fail — this mirrors
// startLearningConsumer/startTaskCompletionConsumer's "absent config -> not
// started" wiring in cmd/main.go.
func NewNotifier(store SubscriptionStore, notify NotifyClient) (*Notifier, error) {
	if store == nil {
		return nil, errors.New("runwatch: subscription store is required")
	}
	if notify == nil {
		return nil, errors.New("runwatch: notify client is required")
	}
	return &Notifier{store: store, notify: notify, now: time.Now}, nil
}

// Run binds the durable JetStream consumer with manual ack and blocks until
// ctx is cancelled. js must already have RunWatchDurable provisioned on
// RunEventsStream (see nats-provisioner) — this call binds, it never
// self-creates the consumer.
func (n *Notifier) Run(ctx context.Context, js nats.JetStreamContext) error {
	sub, err := js.QueueSubscribe(RunEventSubject, RunWatchDurable, func(msg *nats.Msg) {
		n.handle(ctx, msg)
	}, nats.Bind(RunEventsStream, RunWatchDurable), nats.ManualAck())
	if err != nil {
		return fmt.Errorf("runwatch: bind %s/%s: %w", RunEventsStream, RunWatchDurable, err)
	}
	slog.Info("run-watch notify consumer bound", "stream", RunEventsStream, "durable", RunWatchDurable, "subject", RunEventSubject)
	<-ctx.Done()
	_ = sub.Unsubscribe()
	return nil
}

// handle translates one message's process outcome into ack/Nak. Per-message
// errors are logged; they never propagate out of the JetStream callback.
func (n *Notifier) handle(ctx context.Context, msg *nats.Msg) {
	switch n.process(ctx, msg.Data) {
	case outcomeRetry:
		if err := msg.Nak(); err != nil {
			slog.Warn("runwatch: nak failed", "subject", msg.Subject, "error", err)
		}
	default:
		if err := msg.Ack(); err != nil {
			slog.Warn("runwatch: ack failed", "subject", msg.Subject, "error", err)
		}
	}
}

// process is the pure, testable core: decode -> match -> lookup -> notify ->
// persist. No NATS or database is required to exercise it — both dependencies
// are the interfaces above.
func (n *Notifier) process(ctx context.Context, data []byte) outcome {
	var env envelope.Envelope
	if err := json.Unmarshal(data, &env); err != nil {
		// Poison message — acking avoids an infinite redelivery loop for
		// bytes that will never parse.
		slog.Warn("runwatch: decode run envelope failed", "error", err)
		return outcomeAck
	}
	notifyType, ok := terminalNotificationType(env.EventType)
	if !ok {
		return outcomeAck
	}
	orgID := strings.TrimSpace(env.OrgID)
	runID := runIDFrom(&env)
	if orgID == "" || runID == "" {
		slog.Warn("runwatch: terminal run event missing org_id/run_id", "event_type", env.EventType)
		return outcomeAck
	}

	watchers, err := n.store.PendingWatchers(ctx, orgID, runID)
	if err != nil {
		slog.Warn("runwatch: pending watcher lookup failed", "org_id", orgID, "run_id", runID, "error", err)
		return outcomeRetry
	}
	if len(watchers) == 0 {
		// The overwhelmingly common case: this run had no watchers.
		return outcomeAck
	}

	now := n.now().UTC()
	anyFailed := false
	for _, watcher := range watchers {
		req := notifyclient.Request{
			OrganizationID: orgID,
			IdempotencyKey: idempotencyKey(orgID, runID, watcher.UserID, env.EventType),
			Recipient:      notifyclient.Recipient{Kind: notifyclient.RecipientKindUser, ID: watcher.UserID},
			Type:           notifyType,
			Payload:        notificationPayload(runID, notifyType),
		}
		if err := n.notify.Accept(ctx, req); err != nil {
			slog.Warn("runwatch: notify-core delegation failed", "org_id", orgID, "run_id", runID, "user_id", watcher.UserID, "error", err)
			anyFailed = true
			continue
		}
		if err := n.store.MarkNotified(ctx, watcher.ID, env.EventType, now); err != nil {
			slog.Warn("runwatch: mark notified failed", "watcher_id", watcher.ID, "error", err)
			anyFailed = true
		}
	}
	if anyFailed {
		return outcomeRetry
	}
	return outcomeAck
}

// idempotencyKey is "run-watch:{org_id}:{run_id}:{user_id}:{event_type}" —
// stable across redeliveries of the same terminal event for the same
// watcher, so notification-core's own idempotency-key dedup is a second,
// independent line of defense behind the status='pending' guard.
func idempotencyKey(orgID, runID, userID, eventType string) string {
	return fmt.Sprintf("run-watch:%s:%s:%s:%s", orgID, runID, userID, eventType)
}

// notificationPayload is the notification body for a run-watch notification.
func notificationPayload(runID, notifyType string) map[string]any {
	title, body := notificationCopy(runID, notifyType)
	return map[string]any{
		"run_id": runID,
		"title":  title,
		"body":   body,
	}
}

func notificationCopy(runID, notifyType string) (title, body string) {
	if notifyType == notifyTypeRunFailed {
		return "Run failed", fmt.Sprintf("Run %s failed.", runID)
	}
	return "Run completed", fmt.Sprintf("Run %s finished.", runID)
}

// runIDFrom reads the run id from the payload's run_id field (the field
// orchestrator-core's activities.publishRunEvent always sets), falling back
// to correlation_id, and finally to resource_ref's "run/<id>" or "run:<id>"
// form (the convention sessionreview.ParseRunCompleted relies on for the
// same envelope) — covering every producer convention this subject's
// existing consumers already tolerate, since this consumer must match
// whichever one a given run event actually used.
func runIDFrom(env *envelope.Envelope) string {
	var payload struct {
		RunID string `json:"run_id"`
	}
	if len(env.Payload) > 0 {
		if err := json.Unmarshal(env.Payload, &payload); err == nil {
			if id := strings.TrimSpace(payload.RunID); id != "" {
				return id
			}
		}
	}
	if id := strings.TrimSpace(env.CorrelationID); id != "" {
		return id
	}
	for _, prefix := range []string{"run/", "run:"} {
		if id, ok := strings.CutPrefix(env.ResourceRef, prefix); ok && id != "" {
			return id
		}
	}
	return ""
}

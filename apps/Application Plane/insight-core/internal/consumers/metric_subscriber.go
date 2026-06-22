package consumers

import (
	"context"
	"encoding/json"
	"log"
	"strings"
	"time"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

const (
	metricSubscriberDurable = "insight-core-metric-subscriber"
	applicationSubject      = "velion.application.>"

	// Subject-domain prefixes that disambiguate the producer behind an
	// application event. The bare LifecycleEvent.Type values do not encode
	// their domain, so the subject prefix is the honest source-of-truth for
	// per-producer attribution (surface + source).
	conversationSubjectPrefix = "velion.application.conversation."
	socialSubjectPrefix       = "velion.application.social."

	// metricSource* identify the upstream producer on each recorded metric so
	// the metrics view can attribute counts honestly. NOT a label of intent —
	// each value is the real service that published the source event.
	metricSourceConversation = "conversation-core"
	metricSourceSocial       = "social-core"
)

// MetricRecorder is the narrow ingest surface the subscriber needs.
// *insights.Service satisfies it.
type MetricRecorder interface {
	RecordMetricEvent(ctx context.Context, input insights.IngestMetricEventInput) (*insights.MetricEvent, error)
}

// applicationEvent is the shared application LifecycleEvent wire shape
// (snake_case) that conversation-core and social-core both publish. Unknown
// fields are ignored. The `Type` is the subject minus its domain prefix.
type applicationEvent struct {
	ID         string    `json:"id"`
	Type       string    `json:"type"`
	OrgID      string    `json:"org_id"`
	OccurredAt time.Time `json:"occurred_at"`
}

type metricTarget struct {
	surface string
	metric  string
	source  string
}

// conversationMapping maps a conversation-core event Type (the subject minus
// the `velion.application.conversation.` prefix) to an insight (surface,
// metric, source). Each mapped event contributes a count of 1. Types not in
// this map are skipped — never mapped to a fabricated metric.
var conversationMapping = map[string]metricTarget{
	"ai_action.executed":   {insights.SurfaceInbox, "ai_actions_executed", metricSourceConversation},
	"ai_action.reviewed":   {insights.SurfaceInbox, "ai_actions_reviewed", metricSourceConversation},
	"ticket.created":       {insights.SurfaceInbox, "tickets_created", metricSourceConversation},
	"ticket.suggested":     {insights.SurfaceInbox, "tickets_suggested", metricSourceConversation},
	"ticket.resolved":      {insights.SurfaceInbox, "tickets_resolved", metricSourceConversation},
	"conversation.created": {insights.SurfaceInbox, "conversations_created", metricSourceConversation},
	"message.received":     {insights.SurfaceInbox, "messages_received", metricSourceConversation},
	"message.sent":         {insights.SurfaceInbox, "messages_sent", metricSourceConversation},
}

// socialMapping maps a social-core lifecycle event Type (the subject minus the
// `velion.application.social.` prefix, e.g. `post.created`,
// `publish_job.completed`) to an insight (surface=social). Types not in this
// map are skipped — never mapped to a fabricated metric. Mirrors the subject
// constants in social-core/internal/social/types.go.
var socialMapping = map[string]metricTarget{
	"campaign.created":      {insights.SurfaceSocial, "campaigns_created", metricSourceSocial},
	"post.created":          {insights.SurfaceSocial, "posts_created", metricSourceSocial},
	"post.scheduled":        {insights.SurfaceSocial, "posts_scheduled", metricSourceSocial},
	"approval.requested":    {insights.SurfaceSocial, "approvals_requested", metricSourceSocial},
	"approval.decided":      {insights.SurfaceSocial, "approvals_decided", metricSourceSocial},
	"publish_job.queued":    {insights.SurfaceSocial, "publish_jobs_queued", metricSourceSocial},
	"publish_job.completed": {insights.SurfaceSocial, "posts_published", metricSourceSocial},
	"publish_job.failed":    {insights.SurfaceSocial, "publish_jobs_failed", metricSourceSocial},
	"publish_job.blocked":   {insights.SurfaceSocial, "publish_jobs_blocked", metricSourceSocial},
	// `account.synced` is an integration heartbeat, not a surface activity —
	// intentionally NOT mapped so it never inflates the social counts.
}

// MetricSubscriber consumes conversation-core AND social-core application
// events off JetStream and records them as insight metric events — two of the
// real producers behind the metrics view (the model-plane-agents producer is a
// separate subscriber on the model-plane bus). Each producer is attributed by
// the event's subject domain, so counts stay honest per surface.
type MetricSubscriber struct {
	consumer *DurableConsumer
	recorder MetricRecorder
}

func NewMetricSubscriber(js nats.JetStreamContext, recorder MetricRecorder) *MetricSubscriber {
	return &MetricSubscriber{
		consumer: NewDurableConsumer(js, "metric-subscriber"),
		recorder: recorder,
	}
}

func (s *MetricSubscriber) Start(_ context.Context) error {
	return s.consumer.Bind(applicationSubject, metricSubscriberDurable, s.handle)
}

func (s *MetricSubscriber) Stop() { s.consumer.Stop() }

type outcome int

const (
	outcomeAck outcome = iota
	outcomeRetry
)

func (s *MetricSubscriber) handle(msg *nats.Msg) {
	var ev applicationEvent
	if err := json.Unmarshal(msg.Data, &ev); err != nil {
		log.Printf("[insight-core/metric-subscriber] decode %s: %v", msg.Subject, err)
		_ = msg.Ack() // poison message — ack to avoid an infinite redelivery loop
		return
	}
	switch s.process(context.Background(), msg.Subject, ev) {
	case outcomeRetry:
		if err := msg.Nak(); err != nil {
			log.Printf("[insight-core/metric-subscriber] nak: %v", err)
		}
	default:
		if err := msg.Ack(); err != nil {
			log.Printf("[insight-core/metric-subscriber] ack: %v", err)
		}
	}
}

// metricEventID derives a stable metric id from the source event id + metric so a
// duplicate JetStream delivery resolves to the same row (RecordMetricEvent is
// ON CONFLICT DO NOTHING). Empty when the source has no id — the service then
// derives a deterministic id from the event fields.
func metricEventID(eventID, metric string) string {
	if eventID == "" {
		return ""
	}
	return "ins_evt_" + eventID + "_" + metric
}

// resolveTarget selects the (surface, metric, source) for an application event
// from the producer that published it. The subject domain is the
// source-of-truth: a `velion.application.social.*` subject resolves against the
// social mapping, `velion.application.conversation.*` against the conversation
// mapping. Any other subject domain, or a type not in the resolved domain's
// allow-list, returns ok=false — the event is skipped, never counted.
func resolveTarget(subject string, eventType string) (metricTarget, bool) {
	switch {
	case strings.HasPrefix(subject, conversationSubjectPrefix):
		target, ok := conversationMapping[eventType]
		return target, ok
	case strings.HasPrefix(subject, socialSubjectPrefix):
		target, ok := socialMapping[eventType]
		return target, ok
	default:
		return metricTarget{}, false
	}
}

// process maps one application event to a metric and records it. Returns whether
// the message should be acked. Testable without NATS.
func (s *MetricSubscriber) process(ctx context.Context, subject string, ev applicationEvent) outcome {
	target, ok := resolveTarget(subject, ev.Type)
	if !ok {
		return outcomeAck // not a metric-bearing event/domain — skip
	}
	if ev.OrgID == "" {
		return outcomeAck // cannot attribute without an org — skip
	}
	if _, err := s.recorder.RecordMetricEvent(ctx, insights.IngestMetricEventInput{
		ID:         metricEventID(ev.ID, target.metric),
		OrgID:      ev.OrgID,
		Surface:    target.surface,
		Metric:     target.metric,
		Value:      1,
		Unit:       "count",
		Source:     target.source,
		OccurredAt: ev.OccurredAt,
	}); err != nil {
		log.Printf("[insight-core/metric-subscriber] record %s for org %s: %v", target.metric, ev.OrgID, err)
		return outcomeRetry
	}
	return outcomeAck
}

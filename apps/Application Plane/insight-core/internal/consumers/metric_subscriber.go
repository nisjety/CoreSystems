package consumers

import (
	"context"
	"encoding/json"
	"log"
	"time"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

const (
	metricSubscriberDurable = "insight-core-metric-subscriber"
	applicationSubject      = "velion.application.>"
	metricSource            = "conversation-core"
)

// MetricRecorder is the narrow ingest surface the subscriber needs.
// *insights.Service satisfies it.
type MetricRecorder interface {
	RecordMetricEvent(ctx context.Context, input insights.IngestMetricEventInput) (*insights.MetricEvent, error)
}

// applicationEvent is the conversation-core LifecycleEvent wire shape (snake_case)
// that this subscriber maps to metric events. Unknown fields are ignored.
type applicationEvent struct {
	ID         string    `json:"id"`
	Type       string    `json:"type"`
	OrgID      string    `json:"org_id"`
	OccurredAt time.Time `json:"occurred_at"`
}

type metricTarget struct {
	surface string
	metric  string
}

// metricMapping maps a conversation-core event Type (the subject minus the
// `velion.application.conversation.` prefix) to an insight (surface, metric).
// Each mapped event contributes a count of 1. Types not in this map are skipped
// — never mapped to a fabricated metric.
var metricMapping = map[string]metricTarget{
	"ai_action.executed":   {insights.SurfaceInbox, "ai_actions_executed"},
	"ai_action.reviewed":   {insights.SurfaceInbox, "ai_actions_reviewed"},
	"ticket.created":       {insights.SurfaceInbox, "tickets_created"},
	"ticket.suggested":     {insights.SurfaceInbox, "tickets_suggested"},
	"ticket.resolved":      {insights.SurfaceInbox, "tickets_resolved"},
	"conversation.created": {insights.SurfaceInbox, "conversations_created"},
	"message.received":     {insights.SurfaceInbox, "messages_received"},
	"message.sent":         {insights.SurfaceInbox, "messages_sent"},
}

// MetricSubscriber consumes conversation-core application events off JetStream
// and records them as insight metric events — the real producer that backs the
// metrics view (Phase-1 left metrics as an explicit empty-state with no producer).
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
	switch s.process(context.Background(), ev) {
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

// process maps one application event to a metric and records it. Returns whether
// the message should be acked. Testable without NATS.
func (s *MetricSubscriber) process(ctx context.Context, ev applicationEvent) outcome {
	target, ok := metricMapping[ev.Type]
	if !ok {
		return outcomeAck // not a metric-bearing event — skip
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
		Source:     metricSource,
		OccurredAt: ev.OccurredAt,
	}); err != nil {
		log.Printf("[insight-core/metric-subscriber] record %s for org %s: %v", target.metric, ev.OrgID, err)
		return outcomeRetry
	}
	return outcomeAck
}

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
	agentRunSubscriberDurable      = "insight-core-agent-run-subscriber"
	agentApprovalSubscriberDurable = "insight-core-agent-approval-subscriber"

	// Model Plane v1 subjects. Run lifecycle events flow on the per-run
	// `mp.v1.run.{id}.event` subject (wildcard below); orchestration approval
	// state-changes on a single orchestration subject. Both carry the canonical
	// Model Plane Envelope (see apps/Model Plane/go/pkg/envelope/envelope.go and
	// natsx/subjects.go) — insight-core is a separate Go module so the wire shape
	// is duplicated as agentEvent rather than imported across plane boundaries.
	runEventsWildcard       = "mp.v1.run.*.event"
	orchestrationApprovalSj = "mp.v1.orchestration.approval"

	metricSourceModelPlaneAgents = "model-plane-agents"
)

// agentEvent is the subset of the Model Plane canonical Envelope that the
// agents producer needs. Unknown envelope fields (payload, correlation, etc.)
// are ignored. `EventType` is the discriminator (RUN_COMPLETED, …); `Ts` is the
// canonical event timestamp.
type agentEvent struct {
	EventID   string    `json:"event_id"`
	EventType string    `json:"event_type"`
	OrgID     string    `json:"org_id"`
	Ts        time.Time `json:"ts"`
}

// agentMetricMapping maps a Model Plane envelope `event_type` to a
// `surface=agents` insight metric. These are the run/tool/approval lifecycle
// discriminators the execution loop + orchestrator publish (matching the proto
// EventType names in model_plane/v1/events.pb.go). Each mapped event is a count
// of 1. Event types NOT in this map are skipped — never fabricated into a
// metric. ACTION_* are the per-tool steps (tool calls); APPROVAL_* are HITL.
var agentMetricMapping = map[string]metricTarget{
	"RUN_STARTED":        {insights.SurfaceAgents, "agent_runs_started", metricSourceModelPlaneAgents},
	"RUN_COMPLETED":      {insights.SurfaceAgents, "agent_runs_completed", metricSourceModelPlaneAgents},
	"RUN_FAILED":         {insights.SurfaceAgents, "agent_runs_failed", metricSourceModelPlaneAgents},
	"ACTION_COMPLETED":   {insights.SurfaceAgents, "agent_tools_executed", metricSourceModelPlaneAgents},
	"APPROVAL_REQUESTED": {insights.SurfaceAgents, "agent_approvals_requested", metricSourceModelPlaneAgents},
	"APPROVAL_DECIDED":   {insights.SurfaceAgents, "agent_approvals_decided", metricSourceModelPlaneAgents},
}

// AgentSubscriber consumes Model Plane run + approval lifecycle events off the
// model-plane JetStream bus and records them as `surface=agents` metric events
// — the third real producer behind the metrics view. It is a SEPARATE
// subscriber from MetricSubscriber because Model Plane events live on an
// isolated NATS cluster (the model-plane bus), reached via a second connection
// (the model-plane-nats bridge); see cmd/server/main.go.
type AgentSubscriber struct {
	runConsumer      *DurableConsumer
	approvalConsumer *DurableConsumer
	recorder         MetricRecorder
}

func NewAgentSubscriber(js nats.JetStreamContext, recorder MetricRecorder) *AgentSubscriber {
	return &AgentSubscriber{
		runConsumer:      NewDurableConsumer(js, "agent-run-subscriber"),
		approvalConsumer: NewDurableConsumer(js, "agent-approval-subscriber"),
		recorder:         recorder,
	}
}

func (s *AgentSubscriber) Start(_ context.Context) error {
	if err := s.runConsumer.Bind(runEventsWildcard, agentRunSubscriberDurable, s.handle); err != nil {
		return err
	}
	return s.approvalConsumer.Bind(orchestrationApprovalSj, agentApprovalSubscriberDurable, s.handle)
}

func (s *AgentSubscriber) Stop() {
	s.runConsumer.Stop()
	s.approvalConsumer.Stop()
}

func (s *AgentSubscriber) handle(msg *nats.Msg) {
	var ev agentEvent
	if err := json.Unmarshal(msg.Data, &ev); err != nil {
		log.Printf("[insight-core/agent-subscriber] decode %s: %v", msg.Subject, err)
		_ = msg.Ack() // poison message — ack to avoid an infinite redelivery loop
		return
	}
	switch s.process(context.Background(), ev) {
	case outcomeRetry:
		if err := msg.Nak(); err != nil {
			log.Printf("[insight-core/agent-subscriber] nak: %v", err)
		}
	default:
		if err := msg.Ack(); err != nil {
			log.Printf("[insight-core/agent-subscriber] ack: %v", err)
		}
	}
}

// process maps one Model Plane agent event to a metric and records it. The
// envelope `event_type` is the allow-list key — an unmapped type is skipped,
// never counted. Idempotent: the stable metric id derives from the envelope's
// event_id so a duplicate JetStream delivery resolves to the same row. Testable
// without NATS.
func (s *AgentSubscriber) process(ctx context.Context, ev agentEvent) outcome {
	target, ok := agentMetricMapping[ev.EventType]
	if !ok {
		return outcomeAck // not a metric-bearing event type — skip
	}
	if ev.OrgID == "" {
		return outcomeAck // cannot attribute without an org — skip
	}
	if _, err := s.recorder.RecordMetricEvent(ctx, insights.IngestMetricEventInput{
		ID:         metricEventID(ev.EventID, target.metric),
		OrgID:      ev.OrgID,
		Surface:    target.surface,
		Metric:     target.metric,
		Value:      1,
		Unit:       "count",
		Source:     target.source,
		OccurredAt: ev.Ts,
	}); err != nil {
		log.Printf("[insight-core/agent-subscriber] record %s for org %s: %v", target.metric, ev.OrgID, err)
		return outcomeRetry
	}
	return outcomeAck
}

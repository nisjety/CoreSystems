package consumers

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

func agentEvt(eventID, eventType, org string) agentEvent {
	return agentEvent{EventID: eventID, EventType: eventType, OrgID: org, Ts: time.Unix(2, 0).UTC()}
}

func TestAgentProcess_MapsRunCompletedToAgentsSurface(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &AgentSubscriber{recorder: rec}

	if got := sub.process(context.Background(), agentEvt("mp-evt-1", "RUN_COMPLETED", "org-1")); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 1 {
		t.Fatalf("recorded %d, want 1", rec.count())
	}
	in := rec.inputs[0]
	if in.Surface != insights.SurfaceAgents || in.Metric != "agent_runs_completed" || in.Value != 1 || in.OrgID != "org-1" {
		t.Errorf("unexpected agent metric input: %+v", in)
	}
	if in.Source != metricSourceModelPlaneAgents {
		t.Errorf("source = %q, want %q", in.Source, metricSourceModelPlaneAgents)
	}
	if in.ID == "" {
		t.Errorf("metric id should be stable (non-empty) for idempotency")
	}
}

func TestAgentProcess_RecordsUserScopedChatStartFromModelGateway(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &AgentSubscriber{recorder: rec}
	e := agentEvt("mp-chat-1", "RUN_STARTED", "org-1")
	e.UserID = "user-1"
	e.Producer = "model-gateway"
	e.ResourceRef = "request/req-1"

	if got := sub.process(context.Background(), e); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 2 {
		t.Fatalf("recorded %d metrics, want agent and chat", rec.count())
	}
	if rec.inputs[0].ActorUserID != "user-1" {
		t.Errorf("agent metric actor = %q, want user-1", rec.inputs[0].ActorUserID)
	}
	chat := rec.inputs[1]
	if chat.Surface != insights.SurfaceChat || chat.Metric != "chat_turns_started" || chat.ActorUserID != "user-1" {
		t.Errorf("chat metric = %+v, want user-bound chat start", chat)
	}
}

func TestAgentProcess_DropsZeroRetentionEnvelope(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &AgentSubscriber{recorder: rec}
	e := agentEvt("mp-zdr", "RUN_STARTED", "org-1")
	e.ZDR = true

	if got := sub.process(context.Background(), e); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 0 {
		t.Fatalf("zero-retention event produced %d durable metric rows", rec.count())
	}
}

func TestAgentProcess_MapsApprovalAndTool(t *testing.T) {
	cases := []struct {
		eventType  string
		wantMetric string
	}{
		{"RUN_STARTED", "agent_runs_started"},
		{"RUN_FAILED", "agent_runs_failed"},
		{"ACTION_COMPLETED", "agent_tools_executed"},
		{"APPROVAL_REQUESTED", "agent_approvals_requested"},
		{"APPROVAL_DECIDED", "agent_approvals_decided"},
	}
	for _, tc := range cases {
		t.Run(tc.eventType, func(t *testing.T) {
			rec := &fakeRecorder{}
			sub := &AgentSubscriber{recorder: rec}
			if got := sub.process(context.Background(), agentEvt("e", tc.eventType, "org-1")); got != outcomeAck {
				t.Fatalf("outcome = %v, want ack", got)
			}
			if rec.count() != 1 || rec.inputs[0].Surface != insights.SurfaceAgents || rec.inputs[0].Metric != tc.wantMetric {
				t.Errorf("event %q: got %+v, want surface=agents metric=%q", tc.eventType, rec.inputs, tc.wantMetric)
			}
		})
	}
}

func TestAgentProcess_StableIDForDuplicateDelivery(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &AgentSubscriber{recorder: rec}
	e := agentEvt("mp-evt-9", "RUN_COMPLETED", "org-1")

	sub.process(context.Background(), e)
	sub.process(context.Background(), e)

	if rec.count() != 2 {
		t.Fatalf("recorder called %d times, want 2 (dedup is the repo ON CONFLICT, not the subscriber)", rec.count())
	}
	if rec.inputs[0].ID == "" || rec.inputs[0].ID != rec.inputs[1].ID {
		t.Errorf("duplicate delivery must yield the SAME metric id; got %q and %q", rec.inputs[0].ID, rec.inputs[1].ID)
	}
}

// TestAgentProcess_UnmappedEventSkipped locks the allow-list honesty for the
// model-plane-agents leg: an envelope event_type not in the agent mapping (here
// the mid-run RUN_STARTED's sibling ACTION_STARTED, the HITL pause events, and a
// foreign type) must be skipped — never counted, never fabricated.
func TestAgentProcess_UnmappedEventSkipped(t *testing.T) {
	cases := []string{
		"ACTION_STARTED",             // per-tool start; only the completion is counted as an executed tool
		"RUN_PAUSED_FOR_APPROVAL",    // run-state transition, not an agent activity count
		"RUN_RESUMED_AFTER_APPROVAL", // ditto
		"SESSION_START",              // session lifecycle, not a run metric
		"SOME_UNKNOWN_FUTURE_TYPE",   // forward-compatibility: unknown stays uncounted
	}
	for _, eventType := range cases {
		t.Run(eventType, func(t *testing.T) {
			rec := &fakeRecorder{}
			sub := &AgentSubscriber{recorder: rec}
			if got := sub.process(context.Background(), agentEvt("e", eventType, "org-1")); got != outcomeAck {
				t.Fatalf("outcome = %v, want ack (skip)", got)
			}
			if rec.count() != 0 {
				t.Errorf("unmapped agent event_type %q produced a metric (%d) — allow-list violated", eventType, rec.count())
			}
		})
	}
}

func TestAgentProcess_MissingOrgSkipped(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &AgentSubscriber{recorder: rec}
	if got := sub.process(context.Background(), agentEvt("e", "RUN_COMPLETED", "")); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 0 {
		t.Errorf("event without org produced a metric (%d)", rec.count())
	}
}

func TestAgentProcess_RecorderErrorRetries(t *testing.T) {
	rec := &fakeRecorder{err: errors.New("db down")}
	sub := &AgentSubscriber{recorder: rec}
	if got := sub.process(context.Background(), agentEvt("e", "RUN_COMPLETED", "org-1")); got != outcomeRetry {
		t.Fatalf("outcome = %v, want retry", got)
	}
}

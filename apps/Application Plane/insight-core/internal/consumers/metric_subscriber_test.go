package consumers

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

type fakeRecorder struct {
	mu     sync.Mutex
	inputs []insights.IngestMetricEventInput
	err    error
}

func (f *fakeRecorder) RecordMetricEvent(_ context.Context, input insights.IngestMetricEventInput) (*insights.MetricEvent, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.err != nil {
		return nil, f.err
	}
	f.inputs = append(f.inputs, input)
	return &insights.MetricEvent{ID: input.ID, OrgID: input.OrgID, Surface: input.Surface, Metric: input.Metric}, nil
}

func (f *fakeRecorder) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.inputs)
}

func evt(id, typ, org string) applicationEvent {
	return applicationEvent{ID: id, Type: typ, OrgID: org, OccurredAt: time.Unix(1, 0).UTC()}
}

func TestProcess_MapsKnownEventToMetric(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}

	if got := sub.process(context.Background(), evt("evt-1", "ai_action.executed", "org-1")); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 1 {
		t.Fatalf("recorded %d, want 1", rec.count())
	}
	in := rec.inputs[0]
	if in.Surface != insights.SurfaceInbox || in.Metric != "ai_actions_executed" || in.Value != 1 || in.OrgID != "org-1" {
		t.Errorf("unexpected metric input: %+v", in)
	}
	if in.ID == "" {
		t.Errorf("metric id should be stable (non-empty) for idempotency")
	}
}

func TestProcess_StableIDForDuplicateDelivery(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}
	e := evt("evt-9", "ai_action.executed", "org-1")

	sub.process(context.Background(), e)
	sub.process(context.Background(), e)

	if rec.count() != 2 {
		t.Fatalf("recorder called %d times, want 2 (dedup is the repo ON CONFLICT, not the subscriber)", rec.count())
	}
	if rec.inputs[0].ID == "" || rec.inputs[0].ID != rec.inputs[1].ID {
		t.Errorf("duplicate delivery must yield the SAME metric id; got %q and %q", rec.inputs[0].ID, rec.inputs[1].ID)
	}
}

func TestProcess_UnknownTypeSkipped(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}
	if got := sub.process(context.Background(), evt("e", "ticket.linked", "org-1")); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 0 {
		t.Errorf("unknown event type produced a metric (%d)", rec.count())
	}
}

func TestProcess_MissingOrgSkipped(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}
	if got := sub.process(context.Background(), evt("e", "ai_action.executed", "")); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 0 {
		t.Errorf("event without org produced a metric (%d)", rec.count())
	}
}

func TestProcess_RecorderErrorRetries(t *testing.T) {
	rec := &fakeRecorder{err: errors.New("db down")}
	sub := &MetricSubscriber{recorder: rec}
	if got := sub.process(context.Background(), evt("e", "ai_action.executed", "org-1")); got != outcomeRetry {
		t.Fatalf("outcome = %v, want retry", got)
	}
}

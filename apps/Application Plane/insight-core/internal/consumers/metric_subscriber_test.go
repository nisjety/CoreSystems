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

const (
	conversationSubject = conversationSubjectPrefix + "ai_action.executed"
	socialSubject       = socialSubjectPrefix + "publish_job.completed"
)

func evt(id, typ, org string) applicationEvent {
	return applicationEvent{ID: id, Type: typ, OrgID: org, OccurredAt: time.Unix(1, 0).UTC()}
}

func TestProcess_MapsKnownEventToMetric(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}

	if got := sub.process(context.Background(), conversationSubject, evt("evt-1", "ai_action.executed", "org-1")); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 1 {
		t.Fatalf("recorded %d, want 1", rec.count())
	}
	in := rec.inputs[0]
	if in.Surface != insights.SurfaceInbox || in.Metric != "ai_actions_executed" || in.Value != 1 || in.OrgID != "org-1" {
		t.Errorf("unexpected metric input: %+v", in)
	}
	if in.Source != metricSourceConversation {
		t.Errorf("source = %q, want %q", in.Source, metricSourceConversation)
	}
	if in.ID == "" {
		t.Errorf("metric id should be stable (non-empty) for idempotency")
	}
}

func TestProcess_MapsSocialEventToSocialSurface(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}

	if got := sub.process(context.Background(), socialSubject, evt("evt-s", "publish_job.completed", "org-1")); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 1 {
		t.Fatalf("recorded %d, want 1", rec.count())
	}
	in := rec.inputs[0]
	if in.Surface != insights.SurfaceSocial || in.Metric != "posts_published" || in.Value != 1 {
		t.Errorf("unexpected social metric input: %+v", in)
	}
	if in.Source != metricSourceSocial {
		t.Errorf("source = %q, want %q", in.Source, metricSourceSocial)
	}
}

func TestProcess_StableIDForDuplicateDelivery(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}
	e := evt("evt-9", "ai_action.executed", "org-1")

	sub.process(context.Background(), conversationSubject, e)
	sub.process(context.Background(), conversationSubject, e)

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
	if got := sub.process(context.Background(), conversationSubjectPrefix+"ticket.linked", evt("e", "ticket.linked", "org-1")); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 0 {
		t.Errorf("unknown event type produced a metric (%d)", rec.count())
	}
}

// TestProcess_UnmappedEventSkipped locks the allow-list honesty: an event whose
// subject domain is unknown, or whose type is not in the resolved domain's
// allow-list (here the deliberately-unmapped social `account.synced` heartbeat,
// a foreign domain, and cross-domain type/subject mismatches), must be
// skipped — never counted, never fabricated into a metric.
func TestProcess_UnmappedEventSkipped(t *testing.T) {
	cases := []struct {
		name    string
		subject string
		typ     string
	}{
		{"unmapped social heartbeat", socialSubjectPrefix + "account.synced", "account.synced"},
		{"unmapped subject domain", "velion.application.billing.invoice.paid", "invoice.paid"},
		{"social type on conversation domain", conversationSubjectPrefix + "post.created", "post.created"},
		{"conversation type on social domain", socialSubjectPrefix + "message.sent", "message.sent"},
		{"empty subject", "", "ai_action.executed"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rec := &fakeRecorder{}
			sub := &MetricSubscriber{recorder: rec}
			if got := sub.process(context.Background(), tc.subject, evt("e", tc.typ, "org-1")); got != outcomeAck {
				t.Fatalf("outcome = %v, want ack (skip)", got)
			}
			if rec.count() != 0 {
				t.Errorf("unmapped event %q on %q produced a metric (%d) — allow-list violated", tc.typ, tc.subject, rec.count())
			}
		})
	}
}

func TestProcess_MissingOrgSkipped(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}
	if got := sub.process(context.Background(), conversationSubject, evt("e", "ai_action.executed", "")); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 0 {
		t.Errorf("event without org produced a metric (%d)", rec.count())
	}
}

func TestProcess_RecorderErrorRetries(t *testing.T) {
	rec := &fakeRecorder{err: errors.New("db down")}
	sub := &MetricSubscriber{recorder: rec}
	if got := sub.process(context.Background(), conversationSubject, evt("e", "ai_action.executed", "org-1")); got != outcomeRetry {
		t.Fatalf("outcome = %v, want retry", got)
	}
}

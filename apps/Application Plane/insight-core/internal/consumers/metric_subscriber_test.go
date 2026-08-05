package consumers

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/socialmetrics"
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

func TestProcess_AIActionReviewed_ApprovedSplitsToApprovedMetric(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}
	ev := evt("evt-approve", aiActionReviewedType, "org-1")
	ev.Data = map[string]any{"ai_action_id": "aia-1", "decision": "approved"}

	if got := sub.process(context.Background(), conversationSubjectPrefix+aiActionReviewedType, ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 1 {
		t.Fatalf("recorded %d, want 1", rec.count())
	}
	in := rec.inputs[0]
	if in.Surface != insights.SurfaceInbox || in.Metric != "ai_actions_approved" || in.Value != 1 {
		t.Errorf("unexpected metric input: %+v", in)
	}
	if in.Source != metricSourceConversation {
		t.Errorf("source = %q, want %q", in.Source, metricSourceConversation)
	}
}

func TestProcess_AIActionReviewed_RejectedSplitsToRejectedMetric(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}
	ev := evt("evt-reject", aiActionReviewedType, "org-1")
	ev.Data = map[string]any{"ai_action_id": "aia-2", "decision": "rejected"}

	if got := sub.process(context.Background(), conversationSubjectPrefix+aiActionReviewedType, ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 1 {
		t.Fatalf("recorded %d, want 1", rec.count())
	}
	in := rec.inputs[0]
	if in.Surface != insights.SurfaceInbox || in.Metric != "ai_actions_rejected" || in.Value != 1 {
		t.Errorf("unexpected metric input: %+v", in)
	}
}

// TestProcess_AIActionReviewed_UnknownDecisionSkipped locks the honesty
// invariant: a decision outside conversation-core's allowed set ("approved",
// "rejected") must never be guessed into either bucket.
func TestProcess_AIActionReviewed_UnknownDecisionSkipped(t *testing.T) {
	cases := []struct {
		name     string
		decision any
	}{
		{"missing decision key", nil},
		{"empty string", ""},
		{"unrecognized value", "pending"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rec := &fakeRecorder{}
			sub := &MetricSubscriber{recorder: rec}
			ev := evt("evt-x", aiActionReviewedType, "org-1")
			if tc.decision != nil {
				ev.Data = map[string]any{"decision": tc.decision}
			}
			if got := sub.process(context.Background(), conversationSubjectPrefix+aiActionReviewedType, ev); got != outcomeAck {
				t.Fatalf("outcome = %v, want ack (skip)", got)
			}
			if rec.count() != 0 {
				t.Errorf("unknown decision %v produced a metric (%d)", tc.decision, rec.count())
			}
		})
	}
}

func TestProcess_AIActionReviewed_MissingOrgSkipped(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}
	ev := evt("evt-x", aiActionReviewedType, "")
	ev.Data = map[string]any{"decision": "approved"}

	if got := sub.process(context.Background(), conversationSubjectPrefix+aiActionReviewedType, ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 0 {
		t.Errorf("event without org produced a metric (%d)", rec.count())
	}
}

func TestProcess_AIActionReviewed_RecorderErrorRetries(t *testing.T) {
	rec := &fakeRecorder{err: errors.New("db down")}
	sub := &MetricSubscriber{recorder: rec}
	ev := evt("evt-x", aiActionReviewedType, "org-1")
	ev.Data = map[string]any{"decision": "approved"}

	if got := sub.process(context.Background(), conversationSubjectPrefix+aiActionReviewedType, ev); got != outcomeRetry {
		t.Fatalf("outcome = %v, want retry", got)
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
		{"unmapped subject domain", "verevon.application.billing.invoice.paid", "invoice.paid"},
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

type fakeSocialFetcher struct {
	mu             sync.Mutex
	rows           []socialmetrics.Metric
	err            error
	lastOrgID      string
	lastAccountID  string
	lastSnapshotAt time.Time
}

func (f *fakeSocialFetcher) ListMetrics(_ context.Context, orgID, accountID string, snapshotDate time.Time) ([]socialmetrics.Metric, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.lastOrgID, f.lastAccountID, f.lastSnapshotAt = orgID, accountID, snapshotDate
	if f.err != nil {
		return nil, f.err
	}
	return f.rows, nil
}

func metricsSnapshottedEvent(orgID, accountID, snapshotDate string) applicationEvent {
	return applicationEvent{
		ID:    "evt-1",
		Type:  metricsSnapshottedType,
		OrgID: orgID,
		Data: map[string]any{
			"accountId":    accountID,
			"snapshotDate": snapshotDate,
		},
		OccurredAt: time.Unix(1, 0).UTC(),
	}
}

func TestProcess_MetricsSnapshotted_FetchesAndRecordsRealValues(t *testing.T) {
	rec := &fakeRecorder{}
	fetcher := &fakeSocialFetcher{rows: []socialmetrics.Metric{
		{
			OrgID: "org-1", AccountID: "acct-1", ProviderKey: "meta", MetricName: "ads.impressions",
			MetricValue: 4200, Dimensions: map[string]any{"campaign_id": "camp-1"},
			SnapshotDate: time.Date(2026, 7, 4, 0, 0, 0, 0, time.UTC),
		},
		{
			OrgID: "org-1", AccountID: "acct-1", ProviderKey: "meta", MetricName: "ads.spend",
			MetricValue: 15.5, Dimensions: map[string]any{"campaign_id": "camp-1"},
			SnapshotDate: time.Date(2026, 7, 4, 0, 0, 0, 0, time.UTC),
		},
	}}
	sub := &MetricSubscriber{recorder: rec, fetcher: fetcher}

	ev := metricsSnapshottedEvent("org-1", "acct-1", "2026-07-04")
	if got := sub.process(context.Background(), socialSubjectPrefix+metricsSnapshottedType, ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 2 {
		t.Fatalf("recorded %d metrics, want 2", rec.count())
	}
	if fetcher.lastOrgID != "org-1" || fetcher.lastAccountID != "acct-1" {
		t.Errorf("fetch args = (%q,%q), want (org-1,acct-1)", fetcher.lastOrgID, fetcher.lastAccountID)
	}
	for _, input := range rec.inputs {
		if input.Surface != insights.SurfaceExternalAnalytics {
			t.Errorf("surface = %q, want external_analytics", input.Surface)
		}
		if input.ConnectorType != "meta" {
			t.Errorf("connector_type = %q, want meta", input.ConnectorType)
		}
	}
}

func TestProcess_MetricsSnapshotted_NoFetcherConfigured_Skips(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &MetricSubscriber{recorder: rec}

	ev := metricsSnapshottedEvent("org-1", "acct-1", "2026-07-04")
	if got := sub.process(context.Background(), socialSubjectPrefix+metricsSnapshottedType, ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 0 {
		t.Errorf("recorded metrics despite no fetcher configured")
	}
}

func TestProcess_MetricsSnapshotted_MissingAccountID_Skips(t *testing.T) {
	rec := &fakeRecorder{}
	fetcher := &fakeSocialFetcher{}
	sub := &MetricSubscriber{recorder: rec, fetcher: fetcher}

	ev := metricsSnapshottedEvent("org-1", "", "2026-07-04")
	if got := sub.process(context.Background(), socialSubjectPrefix+metricsSnapshottedType, ev); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 0 {
		t.Errorf("recorded metrics despite missing account id")
	}
}

func TestProcess_MetricsSnapshotted_FetchError_Retries(t *testing.T) {
	rec := &fakeRecorder{}
	fetcher := &fakeSocialFetcher{err: errors.New("social-core unavailable")}
	sub := &MetricSubscriber{recorder: rec, fetcher: fetcher}

	ev := metricsSnapshottedEvent("org-1", "acct-1", "2026-07-04")
	if got := sub.process(context.Background(), socialSubjectPrefix+metricsSnapshottedType, ev); got != outcomeRetry {
		t.Fatalf("outcome = %v, want retry on a transient fetch failure", got)
	}
}

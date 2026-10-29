package consumers

import (
	"context"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

func TestIngestionProcess_RecordsCompletedImportWithItsRealDocumentCount(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &IngestionSubscriber{recorder: rec}
	e := ingestionEvent{
		OrgID: "org-1", ImportID: "import-1", Source: "notion",
		DocumentCount: 7, Timestamp: "2026-08-05T10:00:00Z",
	}

	if got := sub.process(context.Background(), "verevon.ingestion.import.completed", e); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 2 {
		t.Fatalf("recorded %d metrics, want completion and document count", rec.count())
	}
	if rec.inputs[0].Surface != insights.SurfaceIngestion || rec.inputs[0].Metric != "imports_completed" {
		t.Errorf("completion metric = %+v", rec.inputs[0])
	}
	if rec.inputs[1].Surface != insights.SurfaceIngestion || rec.inputs[1].Metric != "documents_imported" || rec.inputs[1].Value != 7 {
		t.Errorf("document count metric = %+v", rec.inputs[1])
	}
}

func TestIngestionProcess_SkipsUnknownEventsAndMissingOrganization(t *testing.T) {
	for _, tc := range []struct {
		name    string
		subject string
		event   ingestionEvent
	}{
		{name: "unknown", subject: "verevon.ingestion.import.deleted", event: ingestionEvent{OrgID: "org-1"}},
		{name: "missing org", subject: "verevon.ingestion.import.completed", event: ingestionEvent{}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			rec := &fakeRecorder{}
			sub := &IngestionSubscriber{recorder: rec}
			if got := sub.process(context.Background(), tc.subject, tc.event); got != outcomeAck {
				t.Fatalf("outcome = %v, want ack", got)
			}
			if rec.count() != 0 {
				t.Fatalf("unmapped/unscoped event produced %d metrics", rec.count())
			}
		})
	}
}

func TestIngestionProcess_RecordsIntegrationSyncCompletionWithTenantAndActor(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &IngestionSubscriber{recorder: rec}
	e := ingestionEvent{
		ID:             "evt-sync-1",
		OrganizationID: "org-1",
		UserID:         "user-1",
		CreatedAt:      "2026-08-05T11:00:00Z",
	}

	if got := sub.process(context.Background(), "verevon.ingestion.integration.sync.completed", e); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 1 {
		t.Fatalf("recorded %d metrics, want 1", rec.count())
	}
	metric := rec.inputs[0]
	if metric.Surface != insights.SurfaceIngestion || metric.Metric != "syncs_completed" {
		t.Errorf("metric = %+v, want integration sync completion", metric)
	}
	if metric.OrgID != "org-1" || metric.ActorUserID != "user-1" {
		t.Errorf("scope = org=%q actor=%q, want org-1/user-1", metric.OrgID, metric.ActorUserID)
	}
}

func TestIngestionProcess_RecordsContentFreeKnowledgeLifecycleWithTenantAndActor(t *testing.T) {
	rec := &fakeRecorder{}
	sub := &IngestionSubscriber{recorder: rec}
	e := ingestionEvent{
		ID:          "knowledge-outbox-42",
		OrgID:       "org-1",
		ActorUserID: "user-1",
		Timestamp:   "2026-08-05T11:00:00Z",
	}

	if got := sub.process(context.Background(), "verevon.ingestion.knowledge.document.created", e); got != outcomeAck {
		t.Fatalf("outcome = %v, want ack", got)
	}
	if rec.count() != 1 {
		t.Fatalf("recorded %d metrics, want 1", rec.count())
	}
	metric := rec.inputs[0]
	if metric.Surface != insights.SurfaceKnowledge || metric.Source != "data-plane" || metric.Metric != "documents_created" {
		t.Errorf("metric = %+v, want data-plane knowledge document lifecycle", metric)
	}
	if metric.OrgID != "org-1" || metric.ActorUserID != "user-1" {
		t.Errorf("scope = org=%q actor=%q, want org-1/user-1", metric.OrgID, metric.ActorUserID)
	}
}

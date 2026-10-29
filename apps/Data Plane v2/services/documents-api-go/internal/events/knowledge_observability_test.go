package events

import (
	"encoding/json"
	"errors"
	"testing"
	"time"
)

func TestKnowledgeObservationForOutboxRowProjectsOnlyOrgVisibleNonZDRLifecycle(t *testing.T) {
	createdAt := time.Date(2026, 8, 5, 20, 0, 0, 0, time.UTC)
	row := knowledgeOutboxRow{
		ID:        42,
		OrgID:     "org-1",
		EventType: SubjectDocCreated,
		Payload: []byte(`{
      "document_id":"doc-1",
      "org_id":"org-1",
      "user_id":"user-1",
      "visibility":"org",
      "title":"never leave the Data Plane",
      "source":"notion",
      "zdr":false
    }`),
		CreatedAt: createdAt,
	}

	observation, ok := knowledgeObservationForOutboxRow(row)
	if !ok {
		t.Fatal("org-visible document creation was not projected")
	}
	if observation.Subject != SubjectKnowledgeDocumentCreated {
		t.Fatalf("subject = %q", observation.Subject)
	}
	if observation.ID != "knowledge-outbox-42" || observation.OrgID != "org-1" || observation.ActorUserID != "user-1" {
		t.Fatalf("scope identity = %+v", observation)
	}
	if !observation.OccurredAt.Equal(createdAt) {
		t.Fatalf("occurred_at = %s, want %s", observation.OccurredAt, createdAt)
	}

	encoded, err := json.Marshal(observation)
	if err != nil {
		t.Fatalf("marshal observation: %v", err)
	}
	for _, forbidden := range []string{"title", "source", "doc-1", "notion"} {
		if string(encoded) != "" && contains(string(encoded), forbidden) {
			t.Fatalf("content-bearing field %q escaped: %s", forbidden, encoded)
		}
	}
}

func TestKnowledgeObservationForOutboxRowRejectsPrivateSharedAndZDRDocuments(t *testing.T) {
	base := knowledgeOutboxRow{
		ID:        7,
		OrgID:     "org-1",
		EventType: SubjectDocUpdated,
		CreatedAt: time.Date(2026, 8, 5, 20, 0, 0, 0, time.UTC),
	}
	for name, payload := range map[string]string{
		"private": `{"org_id":"org-1","visibility":"private","zdr":false}`,
		"shared":  `{"org_id":"org-1","visibility":"shared","zdr":false}`,
		"zdr":     `{"org_id":"org-1","visibility":"org","zdr":true}`,
		"legacy":  `{"org_id":"org-1","zdr":false}`,
	} {
		t.Run(name, func(t *testing.T) {
			row := base
			row.Payload = []byte(payload)
			if observation, ok := knowledgeObservationForOutboxRow(row); ok || observation != (KnowledgeObservation{}) {
				t.Fatalf("unsafe row was projected: %+v", observation)
			}
		})
	}
}

func TestKnowledgeObservationForOutboxRowRequiresMatchingTenant(t *testing.T) {
	row := knowledgeOutboxRow{
		ID:        9,
		OrgID:     "org-1",
		EventType: SubjectDocDeleted,
		Payload:   []byte(`{"org_id":"org-2","visibility":"org","zdr":false}`),
		CreatedAt: time.Now().UTC(),
	}
	if observation, ok := knowledgeObservationForOutboxRow(row); ok || observation != (KnowledgeObservation{}) {
		t.Fatalf("cross-tenant payload was projected: %+v", observation)
	}
}

func TestPublishKnowledgeObservationsPublishesOnlySafeContentFreeRows(t *testing.T) {
	publisher := &fakeKnowledgeObservationPublisher{}
	rows := []knowledgeOutboxRow{
		{
			ID: 1, OrgID: "org-1", EventType: SubjectDocCreated,
			Payload:   []byte(`{"org_id":"org-1","user_id":"user-1","visibility":"org","title":"do not publish","zdr":false}`),
			CreatedAt: time.Date(2026, 8, 5, 20, 0, 0, 0, time.UTC),
		},
		{
			ID: 2, OrgID: "org-1", EventType: SubjectDocCreated,
			Payload:   []byte(`{"org_id":"org-1","user_id":"user-1","visibility":"private","zdr":false}`),
			CreatedAt: time.Date(2026, 8, 5, 20, 0, 0, 0, time.UTC),
		},
	}

	published, err := publishKnowledgeObservations(publisher, rows)
	if err != nil {
		t.Fatalf("publish observations: %v", err)
	}
	if len(published) != 2 || published[0] != 1 || published[1] != 2 {
		t.Fatalf("completed rows = %#v, want both the published and intentionally skipped row", published)
	}
	if publisher.flushes != 1 || len(publisher.messages) != 1 {
		t.Fatalf("publisher flushes/messages = %d/%d, want 1/1", publisher.flushes, len(publisher.messages))
	}
	if publisher.messages[0].subject != SubjectKnowledgeDocumentCreated || contains(string(publisher.messages[0].payload), "do not publish") {
		t.Fatalf("unsafe outbound message = %+v", publisher.messages[0])
	}
}

func TestPublishKnowledgeObservationsLeavesAllRowsPendingWhenPublicationFails(t *testing.T) {
	publisher := &fakeKnowledgeObservationPublisher{publishErr: errors.New("application nats unavailable")}
	rows := []knowledgeOutboxRow{{
		ID: 1, OrgID: "org-1", EventType: SubjectDocCreated,
		Payload:   []byte(`{"org_id":"org-1","visibility":"org","zdr":false}`),
		CreatedAt: time.Date(2026, 8, 5, 20, 0, 0, 0, time.UTC),
	}}

	published, err := publishKnowledgeObservations(publisher, rows)
	if err == nil || published != nil {
		t.Fatalf("failure published=%#v err=%v, want no completed rows and error", published, err)
	}
}

type publishedKnowledgeMessage struct {
	payload []byte
	subject string
}

type fakeKnowledgeObservationPublisher struct {
	flushErr   error
	flushes    int
	messages   []publishedKnowledgeMessage
	publishErr error
}

func (f *fakeKnowledgeObservationPublisher) Publish(subject string, payload []byte) error {
	if f.publishErr != nil {
		return f.publishErr
	}
	f.messages = append(f.messages, publishedKnowledgeMessage{subject: subject, payload: append([]byte(nil), payload...)})
	return nil
}

func (f *fakeKnowledgeObservationPublisher) FlushTimeout(_ time.Duration) error {
	f.flushes++
	return f.flushErr
}

func contains(value, token string) bool {
	for offset := 0; offset+len(token) <= len(value); offset++ {
		if value[offset:offset+len(token)] == token {
			return true
		}
	}
	return false
}

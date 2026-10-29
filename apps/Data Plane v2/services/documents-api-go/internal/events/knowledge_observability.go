package events

import (
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// These subjects are owned by the Application Plane's VEREVON_KNOWLEDGE
// stream. They deliberately describe only document lifecycle counts; source
// content, titles, URLs, and document identifiers never leave the Data Plane.
const (
	SubjectKnowledgeDocumentCreated = "verevon.knowledge.document.created"
	SubjectKnowledgeDocumentUpdated = "verevon.knowledge.document.updated"
	SubjectKnowledgeDocumentDeleted = "verevon.knowledge.document.deleted"
)

// KnowledgeObservation is the deliberately narrow cross-plane measurement
// contract. It is safe to persist in Insight Core because it contains no
// document content or source identifier. OrgID is the tenant key and an empty
// ActorUserID represents an automated/system operation, not an unknown user.
type KnowledgeObservation struct {
	ID          string    `json:"id"`
	OrgID       string    `json:"org_id"`
	ActorUserID string    `json:"actor_user_id,omitempty"`
	OccurredAt  time.Time `json:"occurred_at"`
	Subject     string    `json:"-"`
}

// knowledgeOutboxRow is the minimum durable row consumed by the observability
// mirror. Keeping it private prevents other packages from coupling themselves
// to the documents_outbox storage shape.
type knowledgeOutboxRow struct {
	ID        int64
	OrgID     string
	EventType string
	Payload   []byte
	CreatedAt time.Time
}

type documentLifecyclePayload struct {
	OrgID      string `json:"org_id"`
	UserID     string `json:"user_id,omitempty"`
	Visibility string `json:"visibility,omitempty"`
	ZDR        bool   `json:"zdr"`
}

type knowledgeObservationPublisher interface {
	Publish(subject string, payload []byte) error
	FlushTimeout(timeout time.Duration) error
}

const knowledgeObservationFlushTimeout = 5 * time.Second

// publishKnowledgeObservations emits one content-free event per safe outbox
// row and returns every row that can be marked complete. Unsafe rows are
// deliberately marked complete without publishing: retrying a private, ZDR,
// malformed, or mismatched-tenant row cannot make it safe. A remote failure
// returns no completed IDs, keeping the entire batch durable for retry; event
// IDs make a partially accepted batch idempotent in Insight Core.
func publishKnowledgeObservations(publisher knowledgeObservationPublisher, rows []knowledgeOutboxRow) ([]int64, error) {
	if publisher == nil {
		return nil, fmt.Errorf("knowledge observation publisher is unavailable")
	}
	completed := make([]int64, 0, len(rows))
	publishedAny := false
	for _, row := range rows {
		observation, safe := knowledgeObservationForOutboxRow(row)
		if !safe {
			completed = append(completed, row.ID)
			continue
		}
		payload, err := json.Marshal(observation)
		if err != nil {
			return nil, fmt.Errorf("marshal knowledge observation %d: %w", row.ID, err)
		}
		if err := publisher.Publish(observation.Subject, payload); err != nil {
			return nil, fmt.Errorf("publish knowledge observation %d: %w", row.ID, err)
		}
		publishedAny = true
		completed = append(completed, row.ID)
	}
	if publishedAny {
		if err := publisher.FlushTimeout(knowledgeObservationFlushTimeout); err != nil {
			return nil, fmt.Errorf("flush knowledge observations: %w", err)
		}
	}
	return completed, nil
}

// knowledgeObservationForOutboxRow maps a Data Plane document lifecycle row
// to the content-free application event. It is intentionally restrictive:
// only org-visible, non-ZDR documents can contribute to an organization
// roll-up. Private/shared documents remain visible only through the existing
// permission-scoped Knowledge snapshot, never through tenant metrics.
func knowledgeObservationForOutboxRow(row knowledgeOutboxRow) (KnowledgeObservation, bool) {
	if row.ID <= 0 || strings.TrimSpace(row.OrgID) == "" || row.CreatedAt.IsZero() {
		return KnowledgeObservation{}, false
	}
	subject, ok := knowledgeSubjectForDocumentEvent(row.EventType)
	if !ok {
		return KnowledgeObservation{}, false
	}

	var payload documentLifecyclePayload
	if err := json.Unmarshal(row.Payload, &payload); err != nil {
		return KnowledgeObservation{}, false
	}
	if strings.TrimSpace(payload.OrgID) != strings.TrimSpace(row.OrgID) || payload.ZDR || normalizeVisibility(payload.Visibility) != "org" {
		return KnowledgeObservation{}, false
	}

	return KnowledgeObservation{
		ID:          fmt.Sprintf("knowledge-outbox-%d", row.ID),
		OrgID:       strings.TrimSpace(row.OrgID),
		ActorUserID: strings.TrimSpace(payload.UserID),
		OccurredAt:  row.CreatedAt.UTC(),
		Subject:     subject,
	}, true
}

func knowledgeSubjectForDocumentEvent(eventType string) (string, bool) {
	switch strings.TrimSpace(eventType) {
	case SubjectDocCreated:
		return SubjectKnowledgeDocumentCreated, true
	case SubjectDocUpdated:
		return SubjectKnowledgeDocumentUpdated, true
	case SubjectDocDeleted:
		return SubjectKnowledgeDocumentDeleted, true
	default:
		return "", false
	}
}

func normalizeVisibility(value string) string {
	return strings.ToLower(strings.TrimSpace(value))
}

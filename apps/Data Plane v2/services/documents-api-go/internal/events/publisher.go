package events

import (
	"encoding/json"
	"fmt"

	"github.com/nats-io/nats.go"
)

const (
	SubjectDocCreated          = "dataplane.documents.created"
	SubjectDocUpdated          = "dataplane.documents.updated"
	SubjectDocDeleted          = "dataplane.documents.deleted"
	SubjectSourceObjectChanged = "dataplane.source_objects.changed"
	SubjectSourceObjectDeleted = "dataplane.source_objects.deleted"
)

type SourceObjectSubjects struct {
	Changed string
	Deleted string
}

func DefaultSourceObjectSubjects() SourceObjectSubjects {
	return SourceObjectSubjects{
		Changed: SubjectSourceObjectChanged,
		Deleted: SubjectSourceObjectDeleted,
	}
}

type Publisher struct {
	nc *nats.Conn
}

func NewPublisher(nc *nats.Conn) *Publisher {
	return &Publisher{nc: nc}
}

type DocumentCreatedEvent struct {
	DocumentID string `json:"document_id"`
	OrgID      string `json:"org_id"`
	Source     string `json:"source"`
	Type       string `json:"type"`
	Title      string `json:"title"`
}

type DocumentDeletedEvent struct {
	DocumentID string `json:"document_id"`
	OrgID      string `json:"org_id"`
}

func (p *Publisher) PublishDocumentCreated(evt DocumentCreatedEvent) error {
	data, err := json.Marshal(evt)
	if err != nil {
		return fmt.Errorf("marshal doc created event: %w", err)
	}
	return p.nc.Publish(SubjectDocCreated, data)
}

func (p *Publisher) PublishDocumentDeleted(evt DocumentDeletedEvent) error {
	data, err := json.Marshal(evt)
	if err != nil {
		return fmt.Errorf("marshal doc deleted event: %w", err)
	}
	return p.nc.Publish(SubjectDocDeleted, data)
}

// DocumentUpdatedEvent signals that an existing document's content changed
// (re-ingest with the same idempotency key but different content). Consumers
// re-chunk + re-embed by document_id, upserting over the prior vectors/chunks
// so retrieval reflects the new content without leaving a stale duplicate.
type DocumentUpdatedEvent struct {
	DocumentID string `json:"document_id"`
	OrgID      string `json:"org_id"`
	Source     string `json:"source"`
	Type       string `json:"type"`
	Title      string `json:"title"`
}

func (p *Publisher) PublishDocumentUpdated(evt DocumentUpdatedEvent) error {
	data, err := json.Marshal(evt)
	if err != nil {
		return fmt.Errorf("marshal doc updated event: %w", err)
	}
	return p.nc.Publish(SubjectDocUpdated, data)
}

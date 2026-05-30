package model

import (
	"encoding/json"
	"time"
)

type Document struct {
	DocumentID        string          `json:"document_id"`
	OrgID             string          `json:"org_id"`
	Source            string          `json:"source"`
	Type              string          `json:"type"`
	Title             string          `json:"title"`
	Content           string          `json:"content"`
	Status            string          `json:"status"`
	Metadata          json.RawMessage `json:"metadata"`
	ErrorMessage      *string         `json:"error_message,omitempty"`
	ZDRClassification string          `json:"zdr_classification"`
	ZDRReason         *string         `json:"zdr_reason,omitempty"`
	ExtractionTrace   json.RawMessage `json:"extraction_trace,omitempty"`
	CreatedBy         *string         `json:"created_by,omitempty"`
	DeletedBy         *string         `json:"deleted_by,omitempty"`
	CreatedAt         time.Time       `json:"created_at"`
	UpdatedAt         time.Time       `json:"updated_at"`
	DeletedAt         *time.Time      `json:"deleted_at,omitempty"`
}

type CreateDocumentInput struct {
	OrgID             string          `json:"org_id"`
	Source            string          `json:"source"`
	Type              string          `json:"type"`
	Title             string          `json:"title"`
	Content           string          `json:"content"`
	Metadata          json.RawMessage `json:"metadata,omitempty"`
	ZDRClassification string          `json:"zdr_classification,omitempty"`
	ExtractionTrace   json.RawMessage `json:"extraction_trace,omitempty"`
	CreatedBy         string          `json:"created_by,omitempty"`
	IdempotencyKey    string          `json:"idempotency_key,omitempty"`
	IngestPolicy      *IngestPolicy   `json:"ingest_policy,omitempty"`
}

// IngestPolicy mirrors `dataplane.documents.v2.IngestPolicy`. When
// `ZDRMode == "on"` or `EphemeralOnly == true`, the receiver MUST NOT
// persist the document body — only metadata is allowed.
type IngestPolicy struct {
	ZDRMode       string `json:"zdr_mode,omitempty"`
	IndexSchedule string `json:"index_schedule,omitempty"`
	EphemeralOnly bool   `json:"ephemeral_only,omitempty"`
}

// IsZeroRetention returns true when the policy demands ephemeral-only handling.
func (p *IngestPolicy) IsZeroRetention() bool {
	if p == nil {
		return false
	}
	return p.ZDRMode == "on" || p.EphemeralOnly
}

type ListDocumentsInput struct {
	OrgID  string
	Type   string
	Limit  int
	Offset int
}

type ListDocumentsResult struct {
	Documents []Document `json:"documents"`
	Total     int        `json:"total"`
}

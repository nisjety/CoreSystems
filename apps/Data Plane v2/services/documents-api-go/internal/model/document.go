package model

import (
	"encoding/json"
	"strings"
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
	// Per-User Data Ownership & Sharing. OwnerID is the creator (or the system
	// account for grandfathered/non-API rows). Visibility is one of
	// 'private' | 'org' | 'shared'.
	OwnerID    string `json:"owner_id"`
	Visibility string `json:"visibility"`
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
	// OwnerID, when set, stamps the document's owner; otherwise it falls back to
	// CreatedBy, then the org-system account. Visibility (private|org|shared): when
	// empty it defaults to 'private' for an end-user create (a viewer is present)
	// and 'org' for a system/ingest create (no viewer). Setting 'org' explicitly
	// requires an admin scope or a system caller — end users get 403.
	OwnerID    string `json:"owner_id,omitempty"`
	Visibility string `json:"visibility,omitempty"`
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
	mode := strings.ToLower(strings.TrimSpace(p.ZDRMode))
	// Unknown non-empty modes are restrictive by default. Validation can still
	// reject the malformed value, but it must never fall through to persistence.
	return p.EphemeralOnly || (mode != "" && mode != "off")
}

type ListDocumentsInput struct {
	OrgID  string
	Type   string
	Limit  int
	Offset int
	// ViewerID is the requesting user. When empty, the list is org-scoped only
	// (legacy/back-compat). When set, ownership is enforced: a doc is visible if
	// owner_id == ViewerID, visibility IN ('org','shared'), or its id is in
	// GrantedIDs (explicit shares resolved from user-core's resource_grants).
	ViewerID   string
	GrantedIDs []string
}

type ListDocumentsResult struct {
	Documents []Document `json:"documents"`
	Total     int        `json:"total"`
}

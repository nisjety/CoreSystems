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
	// DocumentDate is the source content's own last-modified time (e.g.
	// SharePoint's lastModifiedDateTime), not this row's own CreatedAt/UpdatedAt.
	// Nil means unknown — the retrieval decay stage treats that as no penalty.
	DocumentDate      *time.Time      `json:"document_date,omitempty"`
	CreatedAt         time.Time       `json:"created_at"`
	UpdatedAt         time.Time       `json:"updated_at"`
	DeletedAt         *time.Time      `json:"deleted_at,omitempty"`
	// Per-User Data Ownership & Sharing. OwnerID is the creator (or the system
	// account for grandfathered/non-API rows). Visibility is one of
	// 'private' | 'org' | 'shared'.
	OwnerID    string `json:"owner_id"`
	Visibility string `json:"visibility"`
	// SpaceRef is the Space this document was imported into, when it was
	// imported under a verified Control Space import decision. Nil means it was
	// not — org-wide documents are never retroactively assigned to a room.
	// Data stores the reference opaquely; membership and lifecycle stay with
	// Application and Control.
	SpaceRef *string `json:"space_ref,omitempty"`
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
	// DocumentDate, when supplied, is persisted as-is on create; on an
	// idempotent content refresh it only overwrites the stored value when
	// non-nil (a caller that doesn't know this field must not blank out a
	// previously-known date). See model.Document.DocumentDate.
	DocumentDate      *time.Time      `json:"document_date,omitempty"`
	// OwnerID, when set, stamps the document's owner; otherwise it falls back to
	// CreatedBy, then the org-system account. Visibility (private|org|shared): when
	// empty it defaults to 'private' for an end-user create (a viewer is present)
	// and 'org' for a system/ingest create (no viewer). Setting 'org' explicitly
	// requires an admin scope or a system caller — end users get 403.
	OwnerID    string `json:"owner_id,omitempty"`
	Visibility string `json:"visibility,omitempty"`
	// VisibilityFromSource marks Visibility as a verified connector's reading of
	// the SOURCE system's own ACL, rather than a caller preference. Only the
	// handler sets it (never the wire — hence `json:"-"`), and only for a
	// verified service principal that supplied an explicit value.
	//
	// It is what lets a re-ingest update visibility. The default is to leave
	// visibility untouched on re-ingest so a re-POST cannot silently re-open a
	// private document; but a connector reporting the upstream ACL is the
	// authority on that ACL, and ignoring it would strand documents at whatever
	// visibility they first landed with — including keeping a document
	// org-visible here after it was restricted upstream.
	VisibilityFromSource bool `json:"-"`
	// SpaceRef is set ONLY by the handler, from the verified Space import
	// decision's own `space_ref` — hence `json:"-"`. A body-supplied Space is a
	// claim, not authority, the same way `space_retrieval_bindings` refuses a
	// client-supplied workspace filter as a mapping. Empty means the create
	// carried no Space authority and the column stays NULL.
	SpaceRef string `json:"-"`
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

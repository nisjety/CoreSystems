package events

import "time"

// ItemUpserted is published to subjects.ItemUpserted() whenever a DriveItem
// upsert lands in Postgres. Consumers (Data Plane document ingest, dashboards)
// should treat the absence of hashes as "not a file" rather than "unhashed".
type ItemUpserted struct {
	OrganizationID string    `json:"organization_id"`
	SourceID       string    `json:"source_id"`
	ItemPK         string    `json:"item_pk"`
	ItemID         string    `json:"item_id"`
	ParentItemID   string    `json:"parent_item_id,omitempty"`
	Path           string    `json:"path"`
	Name           string    `json:"name"`
	MimeType       string    `json:"mime_type,omitempty"`
	SizeBytes      int64     `json:"size_bytes"`
	IsFolder       bool      `json:"is_folder"`
	WebURL         string    `json:"web_url,omitempty"`
	QuickXorHash   string    `json:"quick_xor_hash,omitempty"`
	SHA1Hash       string    `json:"sha1_hash,omitempty"`
	Inserted       bool      `json:"inserted"`
	ObservedAt     time.Time `json:"observed_at"`
}

// ItemDeleted signals that Graph returned a deleted facet for an item we had
// previously persisted. The deletion is recorded as a soft-delete in the
// items table; consumers should never treat this as authoritative for
// downstream removal until a human-approved governance workflow runs.
type ItemDeleted struct {
	OrganizationID string    `json:"organization_id"`
	SourceID       string    `json:"source_id"`
	ItemPK         string    `json:"item_pk"`
	ItemID         string    `json:"item_id"`
	Path           string    `json:"path,omitempty"`
	ObservedAt     time.Time `json:"observed_at"`
}

// SourceSynced fires once per successful delta-page chain (when a deltaLink
// has been persisted).
type SourceSynced struct {
	OrganizationID string    `json:"organization_id"`
	SourceID       string    `json:"source_id"`
	ItemsUpserted  int       `json:"items_upserted"`
	ItemsDeleted   int       `json:"items_deleted"`
	StartedAt      time.Time `json:"started_at"`
	CompletedAt    time.Time `json:"completed_at"`
}

// ProposalLifecycle is published on every proposal state transition
// (created / approved / rejected / executed / failed). Consumers — e.g. the
// verevon governance dashboard — use it to reflect review state in real time.
type ProposalLifecycle struct {
	OrganizationID string    `json:"organization_id"`
	ProposalID     string    `json:"proposal_id"`
	Kind           string    `json:"kind"`
	Status         string    `json:"status"`
	Actor          string    `json:"actor,omitempty"`
	ItemCount      int       `json:"item_count"`
	ItemsSucceeded int       `json:"items_succeeded,omitempty"`
	ItemsFailed    int       `json:"items_failed,omitempty"`
	FailureReason  string    `json:"failure_reason,omitempty"`
	OccurredAt     time.Time `json:"occurred_at"`
}

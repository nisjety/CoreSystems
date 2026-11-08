package model

import (
	"encoding/json"
	"time"
)

// LibraryItem is one document in the library read: its identity, provenance
// and metadata, without its text. The text is read by id through Get, under the
// same viewer predicate, so a page never carries document bodies.
type LibraryItem struct {
	DocumentID string          `json:"document_id"`
	Source     string          `json:"source"`
	Type       string          `json:"type"`
	Title      string          `json:"title"`
	Status     string          `json:"status"`
	Metadata   json.RawMessage `json:"metadata"`
	CreatedBy  *string         `json:"created_by,omitempty"`
	OwnerID    string          `json:"owner_id"`
	Visibility string          `json:"visibility"`
	// ContentBytes is the byte length of the stored text, read without the text.
	ContentBytes int64     `json:"content_bytes"`
	CreatedAt    time.Time `json:"created_at"`
	UpdatedAt    time.Time `json:"updated_at"`
}

// ListLibraryInput selects one org's live documents whose metadata origin is
// one of Origins and that belong to no Space. The viewer predicate is the one
// List and Get apply: the owner, org-visible documents, and explicit grants.
type ListLibraryInput struct {
	OrgID      string
	Origins    []string
	Limit      int
	Offset     int
	ViewerID   string
	GrantedIDs []string
}

// ListLibraryResult is one page of the library and the total it pages over.
type ListLibraryResult struct {
	Documents []LibraryItem `json:"documents"`
	Total     int           `json:"total"`
}

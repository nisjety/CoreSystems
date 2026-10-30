// Package leads is the Application-Plane lead-builder domain: filtered
// Enhetsregisteret search (via internal/brreg) plus saved, org-scoped company
// lists and CSV export. COMPANY DATA ONLY — every persisted/serialized record is
// a brreg.Company, which carries no person/role/birth-number (PII) field.
package leads

import (
	"context"
	"errors"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/brreg"
)

var (
	ErrInvalidInput = errors.New("invalid input")
	ErrNotFound     = errors.New("lead list not found")
)

// SavedList is an org-scoped, named set of companies saved from a search.
type SavedList struct {
	ID           string          `json:"id"`
	OrgID        string          `json:"org_id"`
	Name         string          `json:"name"`
	CreatedBy    string          `json:"created_by,omitempty"`
	CompanyCount int             `json:"company_count"`
	Companies    []brreg.Company `json:"companies,omitempty"`
	CreatedAt    time.Time       `json:"created_at"`
	UpdatedAt    time.Time       `json:"updated_at"`
}

// CreateListInput is the request to persist a named list of companies.
type CreateListInput struct {
	OrgID     string
	Name      string
	CreatedBy string
	Companies []brreg.Company
}

// BuildListInput is the request for the governed `leads.build_list` agent tool:
// run a filtered Brreg search, optionally fold in each hit's sub-entities
// (branches), de-duplicate on the canonical organisasjonsnummer, and persist the
// result as a named, org-scoped saved list.
//
// OrgID and CreatedBy are resolved server-side from the authorized identity by
// the caller (the gateway/agent loop) — they are NEVER taken from a client- or
// model-supplied field, so the tool is IDOR-clean.
type BuildListInput struct {
	OrgID           string
	Name            string
	CreatedBy       string
	Filter          brreg.SearchFilter
	IncludeBranches bool
}

// Repository persists saved lead lists. All operations are org-scoped.
type Repository interface {
	CreateList(ctx context.Context, input CreateListInput) (*SavedList, error)
	ListLists(ctx context.Context, orgID string) ([]SavedList, error)
	GetList(ctx context.Context, orgID, listID string) (*SavedList, error)
	DeleteList(ctx context.Context, orgID, listID string) error
}

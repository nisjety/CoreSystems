package leads

import (
	"bytes"
	"context"
	"encoding/csv"
	"fmt"
	"strconv"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/brreg"
)

const maxListCompanies = 10_000

// LeadExportAudit is the per-export audit record (company-level metadata only —
// no company data, no PII).
type LeadExportAudit struct {
	OrgID    string
	ListID   string
	ListName string
	UserID   string
	Count    int
}

// AuditSink records a per-export audit event. Implemented by internal/audit over
// NATS; nil/absent in tests and when NATS is not wired (export still works).
type AuditSink interface {
	PublishLeadExport(ctx context.Context, ev LeadExportAudit)
}

// Service is the lead-builder business logic: filtered Brreg search + saved
// org-scoped company lists + CSV export.
type Service struct {
	repo  Repository
	brreg *brreg.Client
	audit AuditSink
}

func NewService(repo Repository, client *brreg.Client) *Service {
	return &Service{repo: repo, brreg: client}
}

// SetAudit wires the optional per-export audit sink (best-effort).
func (s *Service) SetAudit(a AuditSink) { s.audit = a }

// Search runs a filtered Enhetsregisteret company search. Company data only.
func (s *Service) Search(ctx context.Context, filter brreg.SearchFilter) (*brreg.SearchPage, error) {
	return s.brreg.Search(ctx, filter)
}

// CreateList persists a named, org-scoped list of companies. The companies are
// already company-only (brreg.Company carries no PII); empty names/orgs and
// over-large lists are rejected.
func (s *Service) CreateList(ctx context.Context, input CreateListInput) (*SavedList, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.CreatedBy = strings.TrimSpace(input.CreatedBy)
	if input.OrgID == "" || input.Name == "" {
		return nil, fmt.Errorf("%w: org_id and name are required", ErrInvalidInput)
	}
	if len(input.Companies) == 0 {
		return nil, fmt.Errorf("%w: at least one company is required", ErrInvalidInput)
	}
	if len(input.Companies) > maxListCompanies {
		return nil, fmt.Errorf("%w: a list may hold at most %d companies", ErrInvalidInput, maxListCompanies)
	}
	return s.repo.CreateList(ctx, input)
}

func (s *Service) ListLists(ctx context.Context, orgID string) ([]SavedList, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repo.ListLists(ctx, orgID)
}

func (s *Service) GetList(ctx context.Context, orgID, listID string) (*SavedList, error) {
	orgID, listID = strings.TrimSpace(orgID), strings.TrimSpace(listID)
	if orgID == "" || listID == "" {
		return nil, fmt.Errorf("%w: org_id and list_id are required", ErrInvalidInput)
	}
	return s.repo.GetList(ctx, orgID, listID)
}

func (s *Service) DeleteList(ctx context.Context, orgID, listID string) error {
	orgID, listID = strings.TrimSpace(orgID), strings.TrimSpace(listID)
	if orgID == "" || listID == "" {
		return fmt.Errorf("%w: org_id and list_id are required", ErrInvalidInput)
	}
	return s.repo.DeleteList(ctx, orgID, listID)
}

// csvHeader is the COMPANY-ONLY export schema. There is deliberately no person,
// role, contact, or birth-number column.
var csvHeader = []string{
	"organisasjonsnummer", "navn", "organisasjonsform", "naeringskode",
	"naering_beskrivelse", "kommunenummer", "poststed", "antall_ansatte",
	"registreringsdato", "hjemmeside", "konkurs", "under_avvikling",
}

func companyRow(c brreg.Company) []string {
	ansatte := ""
	if c.AntallAnsatte != nil {
		ansatte = strconv.Itoa(*c.AntallAnsatte)
	}
	return []string{
		c.Organisasjonsnummer, c.Navn, c.Organisasjonsform, c.Naeringskode,
		c.NaeringBeskrivelse, c.Kommunenummer, c.Poststed, ansatte,
		c.Registreringsdato, c.Hjemmeside, strconv.FormatBool(c.Konkurs),
		strconv.FormatBool(c.UnderAvvikling),
	}
}

// ExportCSV renders a saved list as CSV of COMPANY fields only and returns the
// bytes plus the list (for audit metadata). Org-scoped. Emits a best-effort
// per-export audit event (company-count metadata only — no company data, no PII).
func (s *Service) ExportCSV(ctx context.Context, orgID, listID, actorUserID string) ([]byte, *SavedList, error) {
	list, err := s.GetList(ctx, orgID, listID)
	if err != nil {
		return nil, nil, err
	}
	var buf bytes.Buffer
	w := csv.NewWriter(&buf)
	if err := w.Write(csvHeader); err != nil {
		return nil, nil, err
	}
	for _, c := range list.Companies {
		if err := w.Write(companyRow(c)); err != nil {
			return nil, nil, err
		}
	}
	w.Flush()
	if err := w.Error(); err != nil {
		return nil, nil, err
	}

	if s.audit != nil {
		s.audit.PublishLeadExport(ctx, LeadExportAudit{
			OrgID:    orgID,
			ListID:   list.ID,
			ListName: list.Name,
			UserID:   actorUserID,
			Count:    list.CompanyCount,
		})
	}
	return buf.Bytes(), list, nil
}

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

// Branches returns a company's sub-entities/branches (/underenheter). Company
// data only.
func (s *Service) Branches(ctx context.Context, orgnr string) ([]brreg.Branch, error) {
	return s.brreg.Branches(ctx, strings.TrimSpace(orgnr))
}

// Financials returns a company's filed annual accounts (/regnskap). Aggregate
// company figures only.
func (s *Service) Financials(ctx context.Context, orgnr string) ([]brreg.Financials, error) {
	return s.brreg.Financials(ctx, strings.TrimSpace(orgnr))
}

// branchAsCompany maps a sub-entity to the company-only list record. A branch
// has its own organisasjonsnummer, so it is itself a valid lead; it carries no
// PII either.
func branchAsCompany(b brreg.Branch) brreg.Company {
	return brreg.Company{
		Organisasjonsnummer: b.Organisasjonsnummer,
		Navn:                b.Navn,
		Organisasjonsform:   b.Organisasjonsform,
		Naeringskode:        b.Naeringskode,
		NaeringBeskrivelse:  b.NaeringBeskrivelse,
		Kommunenummer:       b.Kommunenummer,
		Poststed:            b.Poststed,
		AntallAnsatte:       b.AntallAnsatte,
		Registreringsdato:   b.Registreringsdato,
	}
}

// BuildList is the governed `leads.build_list` action: run a filtered Brreg
// company search, optionally fold in each hit's sub-entities/branches, de-dupe
// on the canonical organisasjonsnummer, and persist the result as a named,
// org-scoped saved list. COMPANY DATA ONLY.
//
// The org and creator are taken from the input (the caller resolves them
// server-side from the authorized identity), never from the search filter or
// any client/model-supplied field — so this action is IDOR-clean.
func (s *Service) BuildList(ctx context.Context, input BuildListInput) (*SavedList, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.CreatedBy = strings.TrimSpace(input.CreatedBy)
	if input.OrgID == "" || input.Name == "" {
		return nil, fmt.Errorf("%w: org_id and name are required", ErrInvalidInput)
	}
	if err := input.Filter.Validate(); err != nil {
		return nil, fmt.Errorf("%w: %v", ErrInvalidInput, err)
	}

	page, err := s.brreg.Search(ctx, input.Filter)
	if err != nil {
		return nil, err
	}

	lists := [][]brreg.Company{page.Companies}
	if input.IncludeBranches {
		for _, c := range page.Companies {
			branches, berr := s.brreg.Branches(ctx, c.Organisasjonsnummer)
			if berr != nil {
				// Enrichment is best-effort: a branch lookup failure must not
				// discard the primary search result.
				continue
			}
			companyBranches := make([]brreg.Company, 0, len(branches))
			for _, b := range branches {
				companyBranches = append(companyBranches, branchAsCompany(b))
			}
			lists = append(lists, companyBranches)
		}
	}

	companies := brreg.DedupeCompanies(lists...)
	if len(companies) == 0 {
		return nil, fmt.Errorf("%w: the search returned no companies to save", ErrInvalidInput)
	}
	if len(companies) > maxListCompanies {
		companies = companies[:maxListCompanies]
	}

	return s.repo.CreateList(ctx, CreateListInput{
		OrgID:     input.OrgID,
		Name:      input.Name,
		CreatedBy: input.CreatedBy,
		Companies: companies,
	})
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

// sanitizeCSVField guards the export against CSV formula injection: a cell
// whose first character is one a spreadsheet interprets as a formula (= + - @,
// or a leading tab/CR) is prefixed with a single quote so it renders as text.
// Brreg is authoritative company data, but the CSV is opened in Excel/Sheets,
// so this is defense in depth at the export boundary.
func sanitizeCSVField(value string) string {
	if value == "" {
		return value
	}
	switch value[0] {
	case '=', '+', '-', '@', '\t', '\r':
		return "'" + value
	default:
		return value
	}
}

func companyRow(c brreg.Company) []string {
	ansatte := ""
	if c.AntallAnsatte != nil {
		ansatte = strconv.Itoa(*c.AntallAnsatte)
	}
	return []string{
		sanitizeCSVField(c.Organisasjonsnummer), sanitizeCSVField(c.Navn),
		sanitizeCSVField(c.Organisasjonsform), sanitizeCSVField(c.Naeringskode),
		sanitizeCSVField(c.NaeringBeskrivelse), sanitizeCSVField(c.Kommunenummer),
		sanitizeCSVField(c.Poststed), ansatte,
		sanitizeCSVField(c.Registreringsdato), sanitizeCSVField(c.Hjemmeside),
		strconv.FormatBool(c.Konkurs), strconv.FormatBool(c.UnderAvvikling),
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

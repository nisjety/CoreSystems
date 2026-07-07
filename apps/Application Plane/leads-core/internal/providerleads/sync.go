package providerleads

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/integration"
)

// operation names as implemented by integration-corev2's LinkedIn executor
// (internal/actions/service.go: "lead.forms"/"linkedin.lead.forms" needs
// params.owner; "lead.responses"/"linkedin.lead.responses" needs
// params.leadForm; both accept count/start).
const (
	opLeadForms     = "linkedin.lead.forms"
	opLeadResponses = "linkedin.lead.responses"
	pageSize        = 100 // executor clamps count to [10,100]
	maxPages        = 20  // safety valve per form/owner
)

// ActionsGateway is the slice of integration-corev2 the syncer needs.
// Implemented by internal/integration.Client; faked in tests.
type ActionsGateway interface {
	ListConnections(ctx context.Context, orgID, providerKey string) ([]integration.Connection, error)
	ExecuteAction(ctx context.Context, connectionID, operation string, params map[string]any) (json.RawMessage, error)
}

// Syncer pulls LinkedIn lead-form responses for every org whose linkedin
// connection carries the social.leads.read capability and upserts them into
// provider_leads (idempotent on org+provider+lead id).
type Syncer struct {
	gateway ActionsGateway
	repo    Repository
	audit   AuditSink
}

func NewSyncer(gateway ActionsGateway, repo Repository) *Syncer {
	return &Syncer{gateway: gateway, repo: repo}
}

// SetAudit wires the optional per-sync-run audit sink (best-effort).
func (s *Syncer) SetAudit(a AuditSink) { s.audit = a }

// Sync runs one sync pass. orgID == "" discovers every org with a LinkedIn
// connection (internal, unfiltered connection listing) and syncs each.
// ownerOverride, when non-empty, is used as the lead-forms owner URN for every
// connection in this run (manual-trigger escape hatch for connections whose
// providerContext does not carry an owner URN).
func (s *Syncer) Sync(ctx context.Context, orgID, ownerOverride string) (*SyncResult, error) {
	connections, err := s.gateway.ListConnections(ctx, strings.TrimSpace(orgID), ProviderKeyLinkedIn)
	if err != nil {
		// A whole-run gateway failure previously left NO audit trail at all —
		// only per-connection/per-org outcomes were ever published. Emit one
		// so this failure mode is visible instead of silent.
		if s.audit != nil {
			s.audit.PublishProviderLeadSync(ctx, SyncAudit{
				OrgID:       strings.TrimSpace(orgID),
				ProviderKey: ProviderKeyLinkedIn,
				Skipped:     []string{fmt.Sprintf("list linkedin connections: %v", err)},
				Outcome:     "failed",
			})
		}
		return nil, fmt.Errorf("list linkedin connections: %w", err)
	}

	result := &SyncResult{Orgs: []OrgSyncResult{}}

	byOrg := map[string][]integration.Connection{}
	for _, conn := range connections {
		org := strings.TrimSpace(conn.OrganizationID)
		if org == "" {
			continue
		}
		byOrg[org] = append(byOrg[org], conn)
	}
	if len(byOrg) == 0 {
		// Honest skip: no LinkedIn connection anywhere in scope. No provider
		// call is made and nothing is written.
		reason := "no linkedin connections found"
		if strings.TrimSpace(orgID) != "" {
			reason = fmt.Sprintf("no linkedin connections found for org %s", strings.TrimSpace(orgID))
		}
		result.Skipped = append(result.Skipped, reason)
		log.Printf("leads-core: provider-lead sync: %s", reason)
		return result, nil
	}

	orgs := make([]string, 0, len(byOrg))
	for org := range byOrg {
		orgs = append(orgs, org)
	}
	sort.Strings(orgs)

	for _, org := range orgs {
		orgResult := s.syncOrg(ctx, org, byOrg[org], ownerOverride)
		result.Orgs = append(result.Orgs, orgResult)
		result.Connections += orgResult.Connections
		result.Forms += orgResult.Forms
		result.LeadsFetched += orgResult.LeadsFetched
		result.LeadsUpserted += orgResult.LeadsUpserted
		result.Skipped = append(result.Skipped, orgResult.Skipped...)

		if s.audit != nil {
			outcome := "ok"
			if orgResult.Connections == 0 {
				outcome = "skipped"
			}
			s.audit.PublishProviderLeadSync(ctx, SyncAudit{
				OrgID:         org,
				ProviderKey:   ProviderKeyLinkedIn,
				Connections:   orgResult.Connections,
				Forms:         orgResult.Forms,
				LeadsFetched:  orgResult.LeadsFetched,
				LeadsUpserted: orgResult.LeadsUpserted,
				Skipped:       orgResult.Skipped,
				Outcome:       outcome,
			})
		}
	}
	return result, nil
}

// syncOrg syncs one org's eligible connections. Per-connection failures are
// recorded as skips (with reason) instead of failing the whole run.
func (s *Syncer) syncOrg(ctx context.Context, orgID string, connections []integration.Connection, ownerOverride string) OrgSyncResult {
	result := OrgSyncResult{OrgID: orgID, ProviderKey: ProviderKeyLinkedIn}

	for _, conn := range connections {
		if !hasCapability(conn.Capabilities, CapabilityLeadsRead) {
			result.Skipped = append(result.Skipped,
				fmt.Sprintf("connection %s: missing %s capability", conn.ID, CapabilityLeadsRead))
			continue
		}
		owner := resolveOwner(conn, ownerOverride)
		if owner == "" {
			result.Skipped = append(result.Skipped,
				fmt.Sprintf("connection %s: no lead-forms owner urn resolvable (set providerContext ownerUrn/organizationUrn or pass owner on the manual trigger)", conn.ID))
			continue
		}

		forms, err := s.fetchForms(ctx, conn.ID, owner)
		if err != nil {
			result.Skipped = append(result.Skipped, fmt.Sprintf("connection %s: %v", conn.ID, err))
			continue
		}
		result.Connections++
		result.Forms += len(forms)

		leads := make([]ProviderLead, 0, 32)
		for _, form := range forms {
			responses, err := s.fetchResponses(ctx, conn.ID, form)
			if err != nil {
				result.Skipped = append(result.Skipped,
					fmt.Sprintf("connection %s form %s: %v", conn.ID, form.ID, err))
				continue
			}
			for _, response := range responses {
				leads = append(leads, ProviderLead{
					OrgID:          orgID,
					ConnectionID:   conn.ID,
					ProviderKey:    ProviderKeyLinkedIn,
					ProviderLeadID: response.ID,
					FormID:         form.ID,
					FormName:       form.Name,
					SubmittedAt:    response.SubmittedAt,
					Fields:         response.Fields,
				})
			}
		}
		result.LeadsFetched += len(leads)

		upserted, err := s.repo.UpsertLeads(ctx, leads)
		if err != nil {
			result.Skipped = append(result.Skipped, fmt.Sprintf("connection %s: persist: %v", conn.ID, err))
			continue
		}
		result.LeadsUpserted += upserted
	}
	return result
}

// leadForm is one LinkedIn Lead Gen form as returned by linkedin.lead.forms
// (GET /rest/leadForms?q=owner — the executor passes the provider body
// through verbatim).
type leadForm struct {
	ID   string
	URN  string
	Name string
}

// leadResponse is one lead-form response as returned by
// linkedin.lead.responses (GET /rest/leadFormResponses?q=leadForm).
type leadResponse struct {
	ID          string
	SubmittedAt *time.Time
	Fields      json.RawMessage
}

func (s *Syncer) fetchForms(ctx context.Context, connectionID, owner string) ([]leadForm, error) {
	forms := []leadForm{}
	for page := 0; page < maxPages; page++ {
		raw, err := s.gateway.ExecuteAction(ctx, connectionID, opLeadForms, map[string]any{
			"owner": owner,
			"count": pageSize,
			"start": strconv.Itoa(page * pageSize),
		})
		if err != nil {
			return nil, err
		}
		elements, err := decodeElements(raw)
		if err != nil {
			return nil, fmt.Errorf("decode lead forms: %w", err)
		}
		for _, element := range elements {
			id := stringField(element, "id")
			if id == "" {
				continue
			}
			forms = append(forms, leadForm{
				ID:   id,
				URN:  leadFormURN(id),
				Name: firstString(element, "name", "title"),
			})
		}
		if len(elements) < pageSize {
			break
		}
	}
	return forms, nil
}

func (s *Syncer) fetchResponses(ctx context.Context, connectionID string, form leadForm) ([]leadResponse, error) {
	responses := []leadResponse{}
	for page := 0; page < maxPages; page++ {
		raw, err := s.gateway.ExecuteAction(ctx, connectionID, opLeadResponses, map[string]any{
			"leadForm": form.URN,
			"count":    pageSize,
			"start":    strconv.Itoa(page * pageSize),
		})
		if err != nil {
			return nil, err
		}
		elements, err := decodeElements(raw)
		if err != nil {
			return nil, fmt.Errorf("decode lead responses: %w", err)
		}
		for _, element := range elements {
			id := stringField(element, "id")
			if id == "" {
				continue
			}
			responses = append(responses, leadResponse{
				ID:          id,
				SubmittedAt: submittedAt(element),
				Fields:      answerFields(element),
			})
		}
		if len(elements) < pageSize {
			break
		}
	}
	return responses, nil
}

// decodeElements unwraps LinkedIn's collection envelope {"elements":[...]}.
func decodeElements(raw json.RawMessage) ([]map[string]any, error) {
	if len(raw) == 0 {
		return nil, nil
	}
	var payload struct {
		Elements []map[string]any `json:"elements"`
	}
	if err := json.Unmarshal(raw, &payload); err != nil {
		return nil, err
	}
	return payload.Elements, nil
}

// leadFormURN builds the leadForm finder value the executor forwards. Bare
// numeric form ids become urn:li:leadForm:{id}; URNs pass through unchanged.
func leadFormURN(id string) string {
	if strings.HasPrefix(id, "urn:") {
		return id
	}
	return "urn:li:leadForm:" + id
}

// submittedAt reads LinkedIn's epoch-millisecond submittedAt, tolerantly.
func submittedAt(element map[string]any) *time.Time {
	value, ok := element["submittedAt"]
	if !ok {
		return nil
	}
	var millis int64
	switch v := value.(type) {
	case float64:
		millis = int64(v)
	case json.Number:
		parsed, err := v.Int64()
		if err != nil {
			return nil
		}
		millis = parsed
	case string:
		parsed, err := strconv.ParseInt(strings.TrimSpace(v), 10, 64)
		if err != nil {
			return nil
		}
		millis = parsed
	default:
		return nil
	}
	if millis <= 0 {
		return nil
	}
	t := time.UnixMilli(millis).UTC()
	return &t
}

// answerFields extracts the raw question/answer pairs. Preference order:
// formResponse.answers (the Lead Sync shape), then top-level answers, then the
// whole element as a defensive fallback so no lead content is silently lost.
func answerFields(element map[string]any) json.RawMessage {
	if formResponse, ok := element["formResponse"].(map[string]any); ok {
		if answers, ok := formResponse["answers"]; ok {
			if raw, err := json.Marshal(answers); err == nil {
				return raw
			}
		}
	}
	if answers, ok := element["answers"]; ok {
		if raw, err := json.Marshal(answers); err == nil {
			return raw
		}
	}
	if raw, err := json.Marshal(element); err == nil {
		return raw
	}
	return json.RawMessage("[]")
}

// resolveOwner finds the lead-forms owner URN for a connection: an explicit
// override wins, then well-known providerContext keys, then a numeric
// providerAccountId is treated as an organization id. Anything else is
// unresolvable and the connection is skipped honestly.
func resolveOwner(conn integration.Connection, override string) string {
	if v := strings.TrimSpace(override); v != "" {
		return normalizeOwner(v)
	}
	for _, key := range []string{"leadFormsOwner", "ownerUrn", "organizationUrn", "organization", "sponsoredAccountUrn", "sponsoredAccount"} {
		if v := strings.TrimSpace(conn.ProviderContext[key]); v != "" {
			return normalizeOwner(v)
		}
	}
	if v := strings.TrimSpace(conn.ProviderAccountID); v != "" && isDigits(v) {
		return "urn:li:organization:" + v
	}
	return ""
}

func normalizeOwner(value string) string {
	if strings.HasPrefix(value, "urn:") {
		return value
	}
	if isDigits(value) {
		return "urn:li:organization:" + value
	}
	return value
}

func isDigits(value string) bool {
	if value == "" {
		return false
	}
	for _, r := range value {
		if r < '0' || r > '9' {
			return false
		}
	}
	return true
}

func hasCapability(capabilities []string, want string) bool {
	for _, capability := range capabilities {
		if strings.EqualFold(strings.TrimSpace(capability), want) {
			return true
		}
	}
	return false
}

func stringField(element map[string]any, key string) string {
	switch v := element[key].(type) {
	case string:
		return strings.TrimSpace(v)
	case float64:
		if v == float64(int64(v)) {
			return strconv.FormatInt(int64(v), 10)
		}
		return strconv.FormatFloat(v, 'f', -1, 64)
	case json.Number:
		return v.String()
	default:
		return ""
	}
}

func firstString(element map[string]any, keys ...string) string {
	for _, key := range keys {
		if v := stringField(element, key); v != "" {
			return v
		}
	}
	return ""
}

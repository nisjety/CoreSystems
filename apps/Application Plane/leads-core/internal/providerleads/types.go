// Package providerleads syncs provider lead-form responses (LinkedIn Lead Gen
// forms, via integration-corev2's actions gateway) into the org-scoped
// provider_leads table.
//
// PII POSTURE — READ BEFORE EXTENDING: leads-core is company-only everywhere
// else. Lead-form answers are PERSON DATA, and this package is the ONLY place
// in the service allowed to touch them. Provider leads must never be joined
// into lead_lists / lead_list_companies, never appear in the metered company
// CSV export, and sync audit events must carry counts only. Erasure is
// org-scoped via Repository.DeleteByOrg (DELETE /api/v1/provider-leads).
package providerleads

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"time"
)

// CapabilityLeadsRead is the integration-corev2 capability a connection must
// carry for the lead.forms / lead.responses operations (the actions gateway
// enforces it server-side; we filter on it client-side to skip honestly).
const CapabilityLeadsRead = "social.leads.read"

// ProviderKeyLinkedIn is the only provider synced today.
const ProviderKeyLinkedIn = "linkedin"

// ProviderLead is one persisted lead-form response. Fields holds the raw
// provider question/answer pairs verbatim (PERSON DATA).
type ProviderLead struct {
	ID             string          `json:"id"`
	OrgID          string          `json:"org_id"`
	ConnectionID   string          `json:"connection_id"`
	ProviderKey    string          `json:"provider_key"`
	ProviderLeadID string          `json:"provider_lead_id"`
	FormID         string          `json:"form_id"`
	FormName       string          `json:"form_name"`
	SubmittedAt    *time.Time      `json:"submitted_at,omitempty"`
	Fields         json.RawMessage `json:"fields"`
	CreatedAt      time.Time       `json:"created_at"`
}

// Repository persists provider leads. Upserts dedupe on
// (org_id, provider_key, provider_lead_id) so re-syncs are idempotent.
type Repository interface {
	UpsertLeads(ctx context.Context, leads []ProviderLead) (int, error)
	DeleteByOrg(ctx context.Context, orgID string) (int64, error)
}

// SyncAudit is the per-run audit record. COUNTS ONLY — it must never carry a
// form answer or any other person data.
type SyncAudit struct {
	OrgID         string
	ProviderKey   string
	Connections   int
	Forms         int
	LeadsFetched  int
	LeadsUpserted int
	Skipped       []string
	Outcome       string
}

// AuditSink records a per-sync-run audit event. Implemented by internal/audit
// over NATS; nil in tests and when NATS is not wired (sync still works).
type AuditSink interface {
	PublishProviderLeadSync(ctx context.Context, ev SyncAudit)
}

// OrgSyncResult reports one org's sync run.
type OrgSyncResult struct {
	OrgID         string   `json:"org_id"`
	ProviderKey   string   `json:"provider_key"`
	Connections   int      `json:"connections"`
	Forms         int      `json:"forms"`
	LeadsFetched  int      `json:"leads_fetched"`
	LeadsUpserted int      `json:"leads_upserted"`
	Skipped       []string `json:"skipped,omitempty"`
}

// SyncResult aggregates a whole run (one org, or all orgs with a LinkedIn
// connection when triggered without an org filter).
type SyncResult struct {
	Orgs          []OrgSyncResult `json:"orgs"`
	Connections   int             `json:"connections"`
	Forms         int             `json:"forms"`
	LeadsFetched  int             `json:"leads_fetched"`
	LeadsUpserted int             `json:"leads_upserted"`
	Skipped       []string        `json:"skipped,omitempty"`
}

// newID returns a prefixed, random, URL-safe id (same shape as internal/leads).
func newID(prefix string) string {
	var b [12]byte
	_, _ = rand.Read(b[:])
	return prefix + "_" + hex.EncodeToString(b[:])
}

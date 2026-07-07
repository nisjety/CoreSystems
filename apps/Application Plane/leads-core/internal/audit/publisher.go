// Package audit publishes leads-core per-export audit events to NATS so
// audit-core (which subscribes velion.audit.v1.>) durably records them.
// Best-effort: a publish failure never fails the export.
package audit

import (
	"context"
	"encoding/json"
	"log"
	"time"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/leads"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/providerleads"
)

// SubjectLeadExport is the audit-core subject for a metered lead CSV export.
const SubjectLeadExport = "velion.audit.v1.application.lead_export"

// SubjectProviderLeadSync is the audit-core subject for a provider lead-form
// sync run (counts only — never form answers, never person data).
const SubjectProviderLeadSync = "velion.audit.v1.application.provider_lead_sync"

// NatsPublisher implements leads.AuditSink over a core NATS connection.
type NatsPublisher struct {
	conn *nats.Conn
}

func Connect(url, token, name string) (*NatsPublisher, error) {
	opts := []nats.Option{nats.Name(name), nats.Timeout(5 * time.Second)}
	if token != "" {
		opts = append(opts, nats.Token(token))
	}
	conn, err := nats.Connect(url, opts...)
	if err != nil {
		return nil, err
	}
	return &NatsPublisher{conn: conn}, nil
}

func (p *NatsPublisher) Close() {
	if p != nil && p.conn != nil {
		p.conn.Close()
	}
}

// PublishLeadExport emits the flat audit-core AuditEvent shape. It carries only
// company-COUNT metadata — never company data, never PII.
func (p *NatsPublisher) PublishLeadExport(_ context.Context, ev leads.LeadExportAudit) {
	if p == nil || p.conn == nil {
		return
	}
	userID := ev.UserID
	if userID == "" {
		userID = "internal-service"
	}
	body, err := json.Marshal(map[string]any{
		"occurred_at": time.Now().UTC().Format(time.RFC3339Nano),
		"org_id":      ev.OrgID,
		"user_id":     userID,
		"plane":       "application",
		"event":       "lead_export",
		"subject":     "lead_list:" + ev.ListID,
		"resource_id": ev.ListID,
		"outcome":     "ok",
		"details":     map[string]any{"list_name": ev.ListName, "count": ev.Count},
	})
	if err != nil {
		return
	}
	if err := p.conn.Publish(SubjectLeadExport, body); err != nil {
		log.Printf("leads-core: lead_export audit publish failed (best-effort): %v", err)
	}
}

// PublishProviderLeadSync emits the flat audit-core AuditEvent shape for one
// org's provider-lead sync run. COUNTS ONLY — form answers (person data) never
// leave the provider_leads table via this event.
func (p *NatsPublisher) PublishProviderLeadSync(_ context.Context, ev providerleads.SyncAudit) {
	if p == nil || p.conn == nil {
		return
	}
	outcome := ev.Outcome
	if outcome == "" {
		outcome = "ok"
	}
	body, err := json.Marshal(map[string]any{
		"occurred_at": time.Now().UTC().Format(time.RFC3339Nano),
		"org_id":      ev.OrgID,
		"user_id":     "internal-service",
		"plane":       "application",
		"event":       "provider_lead_sync",
		"subject":     "provider_leads:" + ev.ProviderKey,
		"resource_id": ev.ProviderKey,
		"outcome":     outcome,
		"details": map[string]any{
			"provider_key":   ev.ProviderKey,
			"connections":    ev.Connections,
			"forms":          ev.Forms,
			"leads_fetched":  ev.LeadsFetched,
			"leads_upserted": ev.LeadsUpserted,
			"skipped":        ev.Skipped,
		},
	})
	if err != nil {
		return
	}
	if err := p.conn.Publish(SubjectProviderLeadSync, body); err != nil {
		log.Printf("leads-core: provider_lead_sync audit publish failed (best-effort): %v", err)
	}
}

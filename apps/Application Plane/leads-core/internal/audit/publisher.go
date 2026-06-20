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
)

// SubjectLeadExport is the audit-core subject for a metered lead CSV export.
const SubjectLeadExport = "velion.audit.v1.application.lead_export"

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

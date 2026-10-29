package audit

import (
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/leads"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/providerleads"
)

func TestV2AuditPayloadsCarryStableProducerIdentity(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 7, 15, 12, 0, 0, 0, time.UTC)

	for name, fixture := range map[string]struct {
		subject string
		payload map[string]any
	}{
		"export": {
			subject: SubjectLeadExport,
			payload: buildLeadExportPayload("lead:export:fixture", now, leads.LeadExportAudit{OrgID: "org-1", ListID: "list-1", Count: 2}),
		},
		"sync": {
			subject: SubjectProviderLeadSync,
			payload: buildProviderLeadSyncPayload("lead:sync:fixture", now, providerleads.SyncAudit{OrgID: "org-1", ProviderKey: "linkedin"}),
		},
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			if !strings.HasPrefix(fixture.subject, "verevon.audit.v2.application.leads-core.") {
				t.Fatalf("subject = %q", fixture.subject)
			}
			if fixture.payload["producer"] != "leads-core" || fixture.payload["event_id"] == "" {
				t.Fatalf("payload lacks producer identity: %#v", fixture.payload)
			}
			if fixture.payload["occurred_at"] != now.Format(time.RFC3339Nano) {
				t.Fatalf("occurred_at = %#v", fixture.payload["occurred_at"])
			}
		})
	}
}

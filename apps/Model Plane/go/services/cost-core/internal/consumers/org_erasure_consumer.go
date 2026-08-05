package consumers

import (
	"context"
	"encoding/json"
	"log"
	"strings"

	"github.com/nats-io/nats.go"
)

// orgErasureDurable is cost-core's durable name for the shared GDPR erasure
// fan-out. Must exactly match the Durable value audit-core's provisioner
// registers server-side (COST_CORE_GDPR_ORG_ERASURE consumer, reported
// alongside this change) — this consumer binds via nats.Bind, which requires
// that durable to already exist, so a name mismatch here silently breaks the
// org-scoped purge without any startup error.
const orgErasureDurable = "cost-core-org-erasure"

// orgErasureSubject is the platform-wide GDPR erasure fan-out subject. org-core
// publishes it on the shared Control-Plane bus (control-shared-nats, stream
// AQENCIA_CONTROLPLANE) both from its explicit hard-delete-organization path
// and from its 30-day auto-purge cron, so this consumer must be
// pre-provisioned on controlSharedStream (see audit-core's provisioner)
// before Start can bind.
const orgErasureSubject = "verevon.gdpr.erasure.requested"

// orgErasureEvent decodes org-core's GDPR erasure fan-out envelope:
// {"subject_type":"organization","subject_id":"<org_id>","org_id":"<org_id>",
// "requested_by":"<actor_id>","ts":"<RFC3339Nano>"}. The same subject also
// carries subject_type "user" and "user_anonymize" for the Per-User Data
// Ownership erasure fan-out other services consume — this consumer only acts
// on "organization"; every other subject_type is a deliberate no-op (ack,
// never purge).
type orgErasureEvent struct {
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	OrgID       string `json:"org_id"`
	RequestedBy string `json:"requested_by"`
	Timestamp   string `json:"ts"`
}

// OrgPurger is the narrow cost-core surface this consumer needs: permanently
// delete every org-scoped cost/usage row for one org. Both ledger
// implementations (postgres.Store, the durable store, and the in-memory
// ledger.Store fallback) satisfy it via ledger.Ledger's PurgeOrg method.
type OrgPurger interface {
	PurgeOrg(ctx context.Context, orgID string) error
}

// OrgErasureConsumer bridges the Control Plane → Model Plane: when org-core
// publishes verevon.gdpr.erasure.requested for an organization, this
// permanently deletes every cost_entries row cost-core holds for that org —
// the org's entire token/cost ledger.
type OrgErasureConsumer struct {
	consumer *DurableConsumer
	purger   OrgPurger
}

func NewOrgErasureConsumer(js nats.JetStreamContext, purger OrgPurger) *OrgErasureConsumer {
	return &OrgErasureConsumer{
		consumer: NewDurableConsumer(js, "org-erasure"),
		purger:   purger,
	}
}

// Start binds the durable consumer on the shared Control-Plane GDPR erasure
// subject. The consumer must already be provisioned on controlSharedStream
// (see the package doc on orgErasureSubject) before this can bind.
func (c *OrgErasureConsumer) Start(_ context.Context) error {
	return c.consumer.BindProvisioned(orgErasureSubject, controlSharedStream, orgErasureDurable, c.handle)
}

// Stop drains the subscription.
func (c *OrgErasureConsumer) Stop() { c.consumer.Stop() }

func (c *OrgErasureConsumer) handle(msg *nats.Msg) {
	var ev orgErasureEvent
	if err := json.Unmarshal(msg.Data, &ev); err != nil {
		// Poison message — ack to avoid an infinite redelivery loop.
		log.Printf("[cost-core/org-erasure] decode %s: %v", msg.Subject, err)
		_ = msg.Ack()
		return
	}
	switch c.process(context.Background(), ev) {
	case outcomeRetry:
		if err := msg.Nak(); err != nil {
			log.Printf("[cost-core/org-erasure] nak: %v", err)
		}
	default:
		if err := msg.Ack(); err != nil {
			log.Printf("[cost-core/org-erasure] ack: %v", err)
		}
	}
}

// process decodes one erasure-requested event and, if it targets an
// organization, purges every org-scoped cost row for it. Events for any other
// subject_type (e.g. "user"/"user_anonymize" — the Per-User Data Ownership
// erasure fan-out) are outside this consumer's authority and ack as a
// definitive no-op: this consumer NEVER triggers an org-wide purge for a
// per-user erasure request. It is the testable core (no NATS required).
//
// Idempotency: PurgeOrg is a plain `DELETE ... WHERE org_id = $1` with no
// compensating insert, so a redelivered event (NATS is at-least-once) simply
// deletes zero rows the second time — safe to process twice for the same org.
func (c *OrgErasureConsumer) process(ctx context.Context, ev orgErasureEvent) outcome {
	if strings.TrimSpace(ev.SubjectType) != "organization" {
		return outcomeAck
	}
	orgID := strings.TrimSpace(ev.OrgID)
	if orgID == "" {
		orgID = strings.TrimSpace(ev.SubjectID)
	}
	if orgID == "" {
		log.Printf("[cost-core/org-erasure] malformed event (missing org_id/subject_id); skipping")
		return outcomeAck
	}
	if err := c.purger.PurgeOrg(ctx, orgID); err != nil {
		log.Printf("[cost-core/org-erasure] purge cost ledger (org=%s): %v", orgID, err)
		return outcomeRetry
	}
	log.Printf("[cost-core/org-erasure] purged cost ledger (org=%s requested_by=%s)", orgID, ev.RequestedBy)
	return outcomeAck
}

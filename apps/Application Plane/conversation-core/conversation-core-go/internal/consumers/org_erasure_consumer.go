package consumers

import (
	"context"
	"encoding/json"
	"log"
	"strings"

	"github.com/nats-io/nats.go"
)

const orgErasureDurable = "conversation-core-org-erasure"

// orgErasureSubject is the platform-wide GDPR erasure fan-out subject.
// org-core publishes it on this shared Control-Plane bus (not the
// application/model/ingestion namespaces) both from its explicit
// hard-delete-organization path and from its 30-day auto-purge cron, so this
// consumer must be pre-provisioned on controlSharedStream — see
// audit-core's provisioner (mirrors documents-api-go's GDPR subscriber,
// Data Plane v2) — before Start can bind.
const orgErasureSubject = "verevon.gdpr.erasure.requested"

// orgErasureEvent decodes org-core's GDPR erasure fan-out envelope:
// {"subject_type":"organization","subject_id":"<org_id>","org_id":"<org_id>",
// "requested_by":"<actor_id>","ts":"<RFC3339Nano>"}. The same subject also
// carries subject_type "user" (and "user_anonymize") for the Per-User Data
// Ownership erasure fan-out other services consume — this consumer only acts
// on "organization".
type orgErasureEvent struct {
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	OrgID       string `json:"org_id"`
	RequestedBy string `json:"requested_by"`
	Timestamp   string `json:"ts"`
}

// OrgPurger is the narrow conversation-core surface this consumer needs:
// hard-delete every org-scoped conversation row for one org. *conversation.Service
// satisfies it.
type OrgPurger interface {
	HardPurgeByOrg(ctx context.Context, orgID string) error
}

// OrgErasureConsumer bridges the Control Plane → Application Plane: when
// org-core publishes verevon.gdpr.erasure.requested for an organization (via
// its explicit hard-delete path or its 30-day auto-purge cron), this
// hard-purges every conversation_* row conversation-core holds for that org —
// conversations, messages, tickets, AI actions/reviews, outbound intents, and
// every other table scoped by org_id.
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
		log.Printf("[cc-go/org-erasure] decode %s: %v", msg.Subject, err)
		_ = msg.Ack()
		return
	}
	switch c.process(context.Background(), ev) {
	case outcomeRetry:
		if err := msg.Nak(); err != nil {
			log.Printf("[cc-go/org-erasure] nak: %v", err)
		}
	default:
		if err := msg.Ack(); err != nil {
			log.Printf("[cc-go/org-erasure] ack: %v", err)
		}
	}
}

// process decodes one erasure-requested event and, if it targets an
// organization, hard-purges every org-scoped conversation row for it. Events
// for any other subject_type (e.g. "user") are outside this consumer's
// authority and ack as a definitive no-op — mirroring how
// webhook_received_consumer treats a provider it does not handle. It is the
// testable core (no NATS required).
//
// Idempotency: HardPurgeByOrg is a set of plain `DELETE ... WHERE org_id =
// $1` statements with no compensating insert, so a redelivered event (NATS is
// at-least-once) simply deletes zero rows the second time — safe to process
// twice for the same org.
func (c *OrgErasureConsumer) process(ctx context.Context, ev orgErasureEvent) outcome {
	if strings.TrimSpace(ev.SubjectType) != "organization" {
		return outcomeAck
	}
	orgID := strings.TrimSpace(ev.OrgID)
	if orgID == "" {
		orgID = strings.TrimSpace(ev.SubjectID)
	}
	if orgID == "" {
		log.Printf("[cc-go/org-erasure] malformed event (missing org_id/subject_id); skipping")
		return outcomeAck
	}
	if err := c.purger.HardPurgeByOrg(ctx, orgID); err != nil {
		log.Printf("[cc-go/org-erasure] purge conversation data (org=%s): %v", orgID, err)
		return outcomeRetry
	}
	log.Printf("[cc-go/org-erasure] purged conversation data (org=%s requested_by=%s)", orgID, ev.RequestedBy)
	return outcomeAck
}

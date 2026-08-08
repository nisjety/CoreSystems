package consumers

import (
	"context"
	"encoding/json"
	"log"
	"strings"

	"github.com/nats-io/nats.go"
)

const interactiveRetentionDurable = "conversation-core-interactive-retention"

// interactiveRetentionSubject carries an org-core event only when an
// organization newly enables interactive Zero Data Retention. It is separate
// from the irreversible GDPR erasure subject: this consumer has authority to
// remove personal drafts only, never durable support records.
const interactiveRetentionSubject = "aqencia.controlplane.org.interactive_retention.enabled"

type interactiveRetentionEvent struct {
	OrgID string `json:"org_id"`
	ZDR   bool   `json:"zdr"`
}

// ConversationDraftPurger is the smallest capability required by the
// retention event. *conversation.Service satisfies it.
type ConversationDraftPurger interface {
	PurgeConversationDraftsByOrg(ctx context.Context, orgID string) error
}

// InteractiveRetentionConsumer applies the prospective ZDR setting to the
// one historic record type Conversation Core may safely remove: an agent's
// personal, unsent draft. It uses the pre-provisioned Control Plane consumer
// so the publisher and subscriber have no topology-administration authority.
type InteractiveRetentionConsumer struct {
	consumer *DurableConsumer
	purger   ConversationDraftPurger
}

func NewInteractiveRetentionConsumer(js nats.JetStreamContext, purger ConversationDraftPurger) *InteractiveRetentionConsumer {
	return &InteractiveRetentionConsumer{
		consumer: NewDurableConsumer(js, "interactive-retention"),
		purger:   purger,
	}
}

func (c *InteractiveRetentionConsumer) Start(_ context.Context) error {
	return c.consumer.BindProvisioned(interactiveRetentionSubject, controlSharedStream, interactiveRetentionDurable, c.handle)
}

func (c *InteractiveRetentionConsumer) Stop() { c.consumer.Stop() }

func (c *InteractiveRetentionConsumer) handle(msg *nats.Msg) {
	var event interactiveRetentionEvent
	if err := json.Unmarshal(msg.Data, &event); err != nil {
		log.Printf("[cc-go/interactive-retention] decode %s: %v", msg.Subject, err)
		_ = msg.Ack()
		return
	}
	if c.process(context.Background(), event) == outcomeRetry {
		if err := msg.Nak(); err != nil {
			log.Printf("[cc-go/interactive-retention] nak: %v", err)
		}
		return
	}
	if err := msg.Ack(); err != nil {
		log.Printf("[cc-go/interactive-retention] ack: %v", err)
	}
}

// process intentionally treats absent/false `zdr` as a no-op. That fail-closed
// shape prevents a malformed or future event version from deleting drafts.
// DELETE WHERE org_id is idempotent, so NATS redelivery is safe.
func (c *InteractiveRetentionConsumer) process(ctx context.Context, event interactiveRetentionEvent) outcome {
	if !event.ZDR {
		return outcomeAck
	}
	orgID := strings.TrimSpace(event.OrgID)
	if orgID == "" {
		log.Printf("[cc-go/interactive-retention] malformed event (missing org_id); skipping")
		return outcomeAck
	}
	if err := c.purger.PurgeConversationDraftsByOrg(ctx, orgID); err != nil {
		log.Printf("[cc-go/interactive-retention] purge drafts (org=%s): %v", orgID, err)
		return outcomeRetry
	}
	log.Printf("[cc-go/interactive-retention] purged personal drafts (org=%s)", orgID)
	return outcomeAck
}

package consumers

import (
	"context"
	"encoding/json"
	"log"
	"strings"

	"github.com/nats-io/nats.go"
)

// supportRecurrenceZDRPurgeDurable is deliberately its own durable name,
// independent of interactive_retention_consumer.go's
// "conversation-core-interactive-retention" — a separate durable on the same
// subject gets its own JetStream delivery cursor, so this consumer can be
// added without touching (or racing) that one.
const supportRecurrenceZDRPurgeDurable = "conversation-core-support-recurrence-zdr-purge"

// SupportRecurrenceCorpusPurger is the smallest capability this consumer
// needs. *conversation.Service satisfies it.
type SupportRecurrenceCorpusPurger interface {
	PurgeSupportRecurrenceCorpusByOrg(ctx context.Context, orgID string) error
}

// SupportRecurrenceZDRPurgeConsumer is the reactive backstop for the
// support-recurrence corpus builder: SupportRecurrenceCorpusBuilder already
// refuses to build a ZDR-enabled org's corpus on its next sweep, but this
// closes the window between "an org enables ZDR" and "the next sweep runs"
// by deleting that org's corpus immediately, mirroring
// InteractiveRetentionConsumer's shape and fail-closed-on-absent semantics.
type SupportRecurrenceZDRPurgeConsumer struct {
	consumer *DurableConsumer
	purger   SupportRecurrenceCorpusPurger
}

func NewSupportRecurrenceZDRPurgeConsumer(js nats.JetStreamContext, purger SupportRecurrenceCorpusPurger) *SupportRecurrenceZDRPurgeConsumer {
	return &SupportRecurrenceZDRPurgeConsumer{
		consumer: NewDurableConsumer(js, "support-recurrence-zdr-purge"),
		purger:   purger,
	}
}

func (c *SupportRecurrenceZDRPurgeConsumer) Start(_ context.Context) error {
	return c.consumer.BindProvisioned(interactiveRetentionSubject, controlSharedStream, supportRecurrenceZDRPurgeDurable, c.handle)
}

func (c *SupportRecurrenceZDRPurgeConsumer) Stop() { c.consumer.Stop() }

func (c *SupportRecurrenceZDRPurgeConsumer) handle(msg *nats.Msg) {
	var event interactiveRetentionEvent
	if err := json.Unmarshal(msg.Data, &event); err != nil {
		log.Printf("[cc-go/support-recurrence-zdr-purge] decode %s: %v", msg.Subject, err)
		_ = msg.Ack()
		return
	}
	if c.process(context.Background(), event) == outcomeRetry {
		if err := msg.Nak(); err != nil {
			log.Printf("[cc-go/support-recurrence-zdr-purge] nak: %v", err)
		}
		return
	}
	if err := msg.Ack(); err != nil {
		log.Printf("[cc-go/support-recurrence-zdr-purge] ack: %v", err)
	}
}

// process treats absent/false zdr as a no-op, the same fail-closed shape as
// InteractiveRetentionConsumer. DELETE WHERE org_id is idempotent, so NATS
// redelivery is safe.
func (c *SupportRecurrenceZDRPurgeConsumer) process(ctx context.Context, event interactiveRetentionEvent) outcome {
	if !event.ZDR {
		return outcomeAck
	}
	orgID := strings.TrimSpace(event.OrgID)
	if orgID == "" {
		log.Printf("[cc-go/support-recurrence-zdr-purge] malformed event (missing org_id); skipping")
		return outcomeAck
	}
	if err := c.purger.PurgeSupportRecurrenceCorpusByOrg(ctx, orgID); err != nil {
		log.Printf("[cc-go/support-recurrence-zdr-purge] purge (org=%s): %v", orgID, err)
		return outcomeRetry
	}
	log.Printf("[cc-go/support-recurrence-zdr-purge] purged corpus (org=%s)", orgID)
	return outcomeAck
}

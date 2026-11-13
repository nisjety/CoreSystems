package nats

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"strconv"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
)

const (
	// OrgErasureDurable is user-core's durable name for the shared GDPR erasure
	// fan-out. It must exactly match the Durable value audit-core's provisioner
	// registers server-side (UserCoreOrgErasureConsumerName). This consumer binds
	// with nats.Bind, which needs that durable to already exist, so a name
	// mismatch silently breaks the org purge without any startup error.
	OrgErasureDurable = "user-core-org-erasure"

	// orgErasureSubject is the platform-wide GDPR erasure fan-out subject that
	// org-core publishes on the shared Control Plane bus, from both its explicit
	// hard-delete path and its 30-day purge cron.
	orgErasureSubject = "verevon.gdpr.erasure.requested"

	// orgErasureStream is the shared Control Plane stream carrying the fan-out.
	orgErasureStream = "CONTROLPLANE"

	// orgErasureSubjectTypeOrganization is the only subject_type this consumer
	// acts on. The same subject also carries "user" and "user_anonymize".
	orgErasureSubjectTypeOrganization = "organization"

	orgErasureNakBackoffBase = 2 * time.Second
	orgErasureNakBackoffMax  = 60 * time.Second
)

// orgErasureEvent decodes org-core's GDPR erasure fan-out envelope:
// {"subject_type":"organization","subject_id":"<org_id>","org_id":"<org_id>",
// "requested_by":"<actor_id>","ts":"<RFC3339Nano>"}.
type orgErasureEvent struct {
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	OrgID       string `json:"org_id"`
}

// OrgGrantEraser is the narrow user-core surface this consumer needs: delete
// every resource grant of one organisation and report how many rows went.
// users.AclRepository.DeleteGrantsForOrg satisfies it.
type OrgGrantEraser interface {
	DeleteGrantsForOrg(ctx context.Context, orgID string) (int64, error)
}

// OrgErasureConsumer bridges org-core's organisation erasure fan-out to
// user-core: when an organisation is purged, Control erases the grants it owns
// for that organisation. No other plane acts as the owner of these rows.
type OrgErasureConsumer struct {
	js     nats.JetStreamContext
	eraser OrgGrantEraser
	sub    *nats.Subscription
}

// NewOrgErasureConsumer builds a consumer over the shared-bus JetStream context
// and the grant eraser.
func NewOrgErasureConsumer(js nats.JetStreamContext, eraser OrgGrantEraser) *OrgErasureConsumer {
	return &OrgErasureConsumer{js: js, eraser: eraser}
}

// Start binds the pre-provisioned durable push/queue consumer. It never creates
// the consumer: the shared-bus identity holds only consumer info and ack on
// this one durable, so audit-core's provisioner must have created it first.
func (c *OrgErasureConsumer) Start(_ context.Context) error {
	if c == nil || c.js == nil {
		return errors.New("org erasure consumer: shared JetStream context is required")
	}
	if c.eraser == nil {
		return errors.New("org erasure consumer: grant eraser is required")
	}
	sub, err := c.js.QueueSubscribe(orgErasureSubject, OrgErasureDurable, c.handle,
		nats.Bind(orgErasureStream, OrgErasureDurable),
		nats.ManualAck(),
	)
	if err != nil {
		return err
	}
	c.sub = sub
	log.Printf("org erasure consumer bound to %s/%s", orgErasureStream, OrgErasureDurable)
	return nil
}

// Stop removes the subscription.
func (c *OrgErasureConsumer) Stop() {
	if c == nil || c.sub == nil {
		return
	}
	if err := c.sub.Unsubscribe(); err != nil {
		log.Printf("org erasure consumer unsubscribe: %v", err)
	}
	c.sub = nil
}

func (c *OrgErasureConsumer) handle(msg *nats.Msg) {
	var ev orgErasureEvent
	if err := json.Unmarshal(msg.Data, &ev); err != nil {
		// Poison message: ack so it is not redelivered forever. Nothing from
		// the payload is logged.
		log.Printf("org erasure consumer: undecodable event on %s; skipping", msg.Subject)
		if ackErr := msg.Ack(); ackErr != nil {
			log.Printf("org erasure consumer: ack: %v", ackErr)
		}
		return
	}
	eventID := orgErasureEventID(msg)
	if retry := c.process(context.Background(), ev, eventID); retry {
		if err := msg.NakWithDelay(orgErasureNakDelay(msg)); err != nil {
			log.Printf("org erasure consumer: nak: %v", err)
		}
		return
	}
	if err := msg.Ack(); err != nil {
		log.Printf("org erasure consumer: ack: %v", err)
	}
}

// process handles one decoded event and reports whether it must be retried.
// Events whose subject_type is not "organization" (the per-user erasure
// fan-out) are outside this consumer's authority and are a definitive no-op.
// The delete is a single idempotent statement, so redelivery is safe. Logs
// carry only the organisation id, the event id and the deleted count.
func (c *OrgErasureConsumer) process(ctx context.Context, ev orgErasureEvent, eventID string) (retry bool) {
	if strings.TrimSpace(ev.SubjectType) != orgErasureSubjectTypeOrganization {
		return false
	}
	orgID := strings.TrimSpace(ev.OrgID)
	if orgID == "" {
		orgID = strings.TrimSpace(ev.SubjectID)
	}
	if orgID == "" {
		log.Printf("org erasure consumer: organization event %s without org_id; skipping", eventID)
		return false
	}
	deleted, err := c.eraser.DeleteGrantsForOrg(ctx, orgID)
	if err != nil {
		log.Printf("org erasure consumer: delete grants failed (org=%s event=%s): %v", orgID, eventID, err)
		return true
	}
	log.Printf("org erasure consumer: deleted grants (org=%s event=%s deleted=%d)", orgID, eventID, deleted)
	return false
}

// orgErasureEventID names the event for logs by its stream sequence, which is
// content-free. It returns "unknown" when JetStream metadata is unavailable.
func orgErasureEventID(msg *nats.Msg) string {
	meta, err := msg.Metadata()
	if err != nil || meta == nil {
		return "unknown"
	}
	return "seq-" + strconv.FormatUint(meta.Sequence.Stream, 10)
}

// orgErasureNakDelay doubles the redelivery delay with each delivery, capped.
func orgErasureNakDelay(msg *nats.Msg) time.Duration {
	delay := orgErasureNakBackoffBase
	meta, err := msg.Metadata()
	if err != nil || meta == nil {
		return delay
	}
	for i := uint64(1); i < meta.NumDelivered && delay < orgErasureNakBackoffMax; i++ {
		delay *= 2
	}
	if delay > orgErasureNakBackoffMax {
		delay = orgErasureNakBackoffMax
	}
	return delay
}

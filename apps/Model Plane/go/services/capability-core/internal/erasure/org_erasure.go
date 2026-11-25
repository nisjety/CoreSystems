// Package erasure is capability-core's organization erasure path. org-core publishes
// verevon.gdpr.erasure.requested when an organization is hard-deleted (the explicit route and the 30-day purge cron),
// and this package's consumer deletes every capability-core row that organization owns: the learning review counters
// (migration 0045) and the registry, policy, skill, plugin, MCP, procedure and watch rows of migrations 0003 to 0045.
//
// The consumer is bound only when CAPABILITY_CORE_ORG_ERASURE_ENABLED is the literal true (cmd/org_erasure.go), so a
// deployment that has not provisioned the durable keeps its behaviour unchanged.
package erasure

import (
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
)

// orgErasureDurable is capability-core's durable on the shared Control-Plane GDPR erasure fan-out. It must match the
// Durable value audit-core's provisioner registers on CONTROLPLANE: nats.Bind attaches to a durable that already
// exists, so a name mismatch would silently stop the purge.
const orgErasureDurable = "capability-core-org-erasure"

// orgErasureSubject is the platform-wide GDPR erasure fan-out subject. org-core publishes it on the shared bus.
const orgErasureSubject = "verevon.gdpr.erasure.requested"

// controlSharedStream carries the Control Plane's cross-plane events, including the erasure fan-out.
const controlSharedStream = "CONTROLPLANE"

// platformScope is the organization id of the platform's own rows (the catalogue, the seeded tools and the default
// policies). An organization erasure never names it, and the purge refuses it, so the platform's rows survive.
const platformScope = "global"

// purgeTimeout bounds one organization purge. The purge is one transaction over every table; a purge that is still
// running after this is rolled back and redelivered.
const purgeTimeout = 2 * time.Minute

// orgErasureEvent decodes org-core's erasure envelope:
// {"subject_type":"organization","subject_id":"<org_id>","org_id":"<org_id>","requested_by":"<actor_id>","ts":"..."}.
// The same subject also carries subject_type "user" and "user_anonymize" for per-user erasure. Those are another
// service's business: an org_id on a per-user event is the person's organization, not a request to erase it.
type orgErasureEvent struct {
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	OrgID       string `json:"org_id"`
	RequestedBy string `json:"requested_by"`
	Timestamp   string `json:"ts"`
}

// Purger deletes every capability-core row one organization owns, in one transaction, and reports how many rows it
// deleted. A repeated call for the same organization deletes nothing and returns zero.
type Purger interface {
	PurgeOrg(ctx context.Context, orgID string) (int64, error)
}

// outcome tells the message handler whether to acknowledge a message (done, or a definitive skip) or ask JetStream to
// redeliver it (a transient failure).
type outcome int

const (
	outcomeAck outcome = iota
	outcomeRetry
)

// OrgErasureConsumer binds the durable push subscription on the shared erasure subject and purges the organization
// each organization-scoped event names. It uses manual acknowledgements, so a purge finishes before the message is
// acked.
type OrgErasureConsumer struct {
	js     nats.JetStreamContext
	purger Purger
	sub    *nats.Subscription
}

// NewOrgErasureConsumer returns a consumer that purges through purger. It does not bind until Start.
func NewOrgErasureConsumer(js nats.JetStreamContext, purger Purger) *OrgErasureConsumer {
	return &OrgErasureConsumer{js: js, purger: purger}
}

// Start binds the pre-provisioned durable on CONTROLPLANE. nats.Bind, not a self-provisioning subscribe, is what keeps
// this identity to bind, fetch and acknowledge on one durable, so it never needs consumer-creation rights.
func (c *OrgErasureConsumer) Start() error {
	if c == nil || c.js == nil {
		return errors.New("erasure: no JetStream context to bind the org erasure consumer")
	}
	sub, err := c.js.QueueSubscribe(orgErasureSubject, orgErasureDurable, c.handle,
		nats.Bind(controlSharedStream, orgErasureDurable),
		nats.ManualAck(),
	)
	if err != nil {
		return err
	}
	c.sub = sub
	return nil
}

// Stop drains the bound subscription.
func (c *OrgErasureConsumer) Stop() {
	if c == nil || c.sub == nil {
		return
	}
	if err := c.sub.Unsubscribe(); err != nil {
		slog.Warn("capability-core org erasure: unsubscribe failed", "error", err)
	}
	c.sub = nil
}

func (c *OrgErasureConsumer) handle(msg *nats.Msg) {
	ev, err := decodeErasureEvent(msg.Data)
	if err != nil {
		// A poison message is acknowledged, so JetStream does not redeliver it forever. The subject and the decode
		// error carry no event content.
		slog.Warn("capability-core org erasure: undecodable event acknowledged", "subject", msg.Subject, "error", err)
		if ackErr := msg.Ack(); ackErr != nil {
			slog.Warn("capability-core org erasure: ack failed", "error", ackErr)
		}
		return
	}
	switch c.process(context.Background(), ev) {
	case outcomeRetry:
		if nakErr := msg.Nak(); nakErr != nil {
			slog.Warn("capability-core org erasure: nak failed", "error", nakErr)
		}
	default:
		if ackErr := msg.Ack(); ackErr != nil {
			slog.Warn("capability-core org erasure: ack failed", "error", ackErr)
		}
	}
}

// process is the testable core of the consumer: it decides whether an event erases an organization and runs the
// purge. It never purges for a per-user event, an event with no organization, or the platform scope.
func (c *OrgErasureConsumer) process(ctx context.Context, ev orgErasureEvent) outcome {
	if strings.TrimSpace(ev.SubjectType) != "organization" {
		return outcomeAck
	}
	orgID, ok := organizationOf(ev)
	if !ok {
		slog.Warn("capability-core org erasure: event names no erasable organization; skipped")
		return outcomeAck
	}
	purgeCtx, cancel := context.WithTimeout(ctx, purgeTimeout)
	defer cancel()
	deleted, err := c.purger.PurgeOrg(purgeCtx, orgID)
	if err != nil {
		slog.Error("capability-core org erasure: purge failed; the event will be redelivered", "org_id", orgID, "error", err)
		return outcomeRetry
	}
	slog.Info("capability-core org erasure: organization purged", "org_id", orgID, "rows_deleted", deleted,
		"requested_by", ev.RequestedBy)
	return outcomeAck
}

// organizationOf returns the organization an organization-scoped event erases. The org_id is preferred and subject_id is
// the fallback. An empty id, or the platform scope, erases nothing.
func organizationOf(ev orgErasureEvent) (string, bool) {
	orgID := strings.TrimSpace(ev.OrgID)
	if orgID == "" {
		orgID = strings.TrimSpace(ev.SubjectID)
	}
	if orgID == "" || orgID == platformScope {
		return "", false
	}
	return orgID, true
}

func decodeErasureEvent(data []byte) (orgErasureEvent, error) {
	var ev orgErasureEvent
	if err := json.Unmarshal(data, &ev); err != nil {
		return orgErasureEvent{}, err
	}
	return ev, nil
}

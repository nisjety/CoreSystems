package nats

import (
	"context"
	"encoding/json"
	"log"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/nats-io/nats.go"
)

// BillingSyncSubscriber mirrors billing-core's account state into this
// service's own organizations.plan column. Before this existed, org-core had
// no consumer of any billing-core event at all: an admin-driven plan change
// made through org-core's own API correctly reached billing-core (via
// UpdatePlanWithOutbox -> organization.plan.changed, consumed by
// billing-core's durable plan-change subscriber), but the reverse never
// happened — a billing-driven change (trial activation, checkout
// confirmation, webhook-driven state change) left organizations.plan stale
// indefinitely. That gap was most visible at trial start, which previously
// published zero plan-change-shaped event at all.
//
// Delivery is plain core NATS (at-most-once), matching the reliability tier
// this exact subject already has elsewhere (session-core's cache
// invalidator). That is an intentional, disclosed trade-off, not an
// oversight: org-core's own 10-minute organization cache TTL is a natural
// backstop, and billing-core republishes billing.account.updated on every
// account write, so a single missed delivery self-heals on the next one.
type BillingSyncSubscriber struct {
	client     *Client
	orgService *orgcore.Service
	sub        *nats.Subscription
}

func NewBillingSyncSubscriber(client *Client, orgService *orgcore.Service) *BillingSyncSubscriber {
	return &BillingSyncSubscriber{client: client, orgService: orgService}
}

func (s *BillingSyncSubscriber) Start(ctx context.Context) error {
	sub, err := s.client.QueueSubscribe(
		SubjectBillingAccountUpdated,
		"org-core-billing-sync",
		s.handle(ctx),
	)
	if err != nil {
		return err
	}
	s.sub = sub
	log.Printf("nats: billing sync subscriber listening on %s", SubjectBillingAccountUpdated)
	return nil
}

func (s *BillingSyncSubscriber) Stop() {
	if s == nil || s.sub == nil {
		return
	}
	if err := s.sub.Drain(); err != nil {
		log.Printf("nats: billing sync subscriber drain failed: %v", err)
	}
}

func (s *BillingSyncSubscriber) handle(ctx context.Context) nats.MsgHandler {
	return func(msg *nats.Msg) {
		var payload map[string]any
		if err := json.Unmarshal(msg.Data, &payload); err != nil {
			log.Printf("nats: billing sync invalid payload: %v", err)
			return
		}
		if nested, ok := payload["data"].(map[string]any); ok {
			payload = nested
		}

		orgID, _ := payload["org_id"].(string)
		plan, _ := payload["plan"].(string)
		if orgID == "" || plan == "" {
			log.Printf("nats: billing sync event missing org_id or plan, skipping")
			return
		}

		skippedOverride, err := s.orgService.ApplyBillingPlanSync(ctx, orgID, plan)
		switch {
		case err != nil:
			log.Printf("nats: billing sync failed for org=%s plan=%s: %v", orgID, plan, err)
		case skippedOverride:
			log.Printf("nats: billing sync skipped for org=%s — manual plan_override in effect", orgID)
		default:
			log.Printf("nats: billing sync applied for org=%s plan=%s", orgID, plan)
		}
	}
}

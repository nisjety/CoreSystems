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
	orgService billingSyncOrgService
	sub        *nats.Subscription
	// zdrSignal, when set (SetZDRSignal), emits the ZDR-enabled event for a
	// billing sync that turns the org's effective ZDR on (X-9). Nil keeps the
	// old behaviour: no posture reads, no extra publish.
	zdrSignal zdrBillingSyncSignaler
}

// billingSyncOrgService is the part of *orgcore.Service the subscriber uses.
type billingSyncOrgService interface {
	ApplyBillingPlanSync(ctx context.Context, orgID, plan string) (bool, error)
	GetRetentionPosture(ctx context.Context, id string) (*orgcore.RetentionPosture, error)
}

// zdrBillingSyncSignaler is satisfied by *SharedPublisher.
type zdrBillingSyncSignaler interface {
	PublishZDREnabledForBillingSync(ctx context.Context, orgID, plan string) error
}

func NewBillingSyncSubscriber(client *Client, orgService *orgcore.Service) *BillingSyncSubscriber {
	return &BillingSyncSubscriber{client: client, orgService: orgService}
}

// SetZDRSignal enables the billing-sync ZDR-enabled signal. A nil or typed-nil
// publisher (shared NATS not configured) leaves it off.
func (s *BillingSyncSubscriber) SetZDRSignal(signal zdrBillingSyncSignaler) {
	if s == nil || signal == nil {
		return
	}
	if sp, ok := signal.(*SharedPublisher); ok && sp == nil {
		return
	}
	s.zdrSignal = signal
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

		before, haveBefore := s.postureBefore(ctx, orgID)
		skippedOverride, err := s.orgService.ApplyBillingPlanSync(ctx, orgID, plan)
		switch {
		case err != nil:
			log.Printf("nats: billing sync failed for org=%s plan=%s: %v", orgID, plan, err)
		case skippedOverride:
			log.Printf("nats: billing sync skipped for org=%s — manual plan_override in effect", orgID)
		default:
			log.Printf("nats: billing sync applied for org=%s plan=%s", orgID, plan)
			if haveBefore {
				s.signalZDRIfTurnedOn(ctx, orgID, plan, before)
			}
		}
	}
}

// postureBefore reads the posture ahead of the sync when the ZDR signal is on.
// A failed read turns the signal off for this delivery (logged): without the
// "before" posture a transition cannot be told from a steady state.
func (s *BillingSyncSubscriber) postureBefore(ctx context.Context, orgID string) (orgcore.RetentionPosture, bool) {
	if s.zdrSignal == nil {
		return orgcore.RetentionPosture{}, false
	}
	posture, err := s.orgService.GetRetentionPosture(ctx, orgID)
	if err != nil || posture == nil {
		log.Printf("nats: billing sync ZDR signal: posture read before sync failed for org=%s: %v", orgID, err)
		return orgcore.RetentionPosture{}, false
	}
	return *posture, true
}

// signalZDRIfTurnedOn emits the ZDR-enabled event when the sync turned the
// org's effective ZDR on (X-9). Failures are logged: this subscriber is core
// NATS at-most-once, and billing-core republishes on every account write, but
// a republish finds the plan already applied (no transition), so a failed emit
// here is not retried. Recorded as a depth item in the X-9 status.
func (s *BillingSyncSubscriber) signalZDRIfTurnedOn(ctx context.Context, orgID, plan string, before orgcore.RetentionPosture) {
	after, err := s.orgService.GetRetentionPosture(ctx, orgID)
	if err != nil || after == nil {
		log.Printf("nats: billing sync ZDR signal: posture read after sync failed for org=%s: %v", orgID, err)
		return
	}
	if !zdrTurnedOn(before, *after) {
		return
	}
	if err := s.zdrSignal.PublishZDREnabledForBillingSync(ctx, orgID, plan); err != nil {
		log.Printf("nats: billing sync ZDR signal: publish failed for org=%s: %v", orgID, err)
		return
	}
	log.Printf("nats: billing sync turned ZDR on for org=%s: ZDR-enabled event published", orgID)
}

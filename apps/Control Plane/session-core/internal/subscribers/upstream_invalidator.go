// Package subscribers wires NATS topics to session-core's cache + republish
// pipeline. G34-followup: subscribe to the canonical "thing changed"
// subjects published by user-core / org-core / billing-core on the shared
// inter-plane-bus, invalidate the Redis cache for the affected user/org
// pair, and re-publish `app.session.entitlements_changed` so
// notification-core fires the user-visible toast.
package subscribers

import (
	"context"
	"encoding/json"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"

	internalnats "github.com/I-Dacosta/CoreSystem/apps/session-core/internal/nats"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/redis"
)

// Upstream subjects. Subscribers are core NATS (not JetStream) — these
// publishes are fire-and-forget on the cores' side, and our invalidation is
// idempotent (DEL on Redis), so at-most-once delivery + the 30s TTL bound
// is the correctness safety net. Missing one delivery just means the cache
// entry expires naturally.
const (
	SubjectUserProfileUpdated    = "user.profile.updated"
	SubjectOrgPlanChanged        = "organization.plan.changed"
	SubjectOrgMemberAdded        = "organization.member.added"
	SubjectOrgMemberRemoved      = "organization.member.removed"
	SubjectOrgUpdated            = "organization.updated"
	SubjectBillingAccountUpdated = "billing.account.updated"
	SubjectBillingPlanChanged    = "billing.plan.changed"
)

// upstreamEvent is the union of fields we care about across all five
// subjects. Fields are pointers so absence is observable — none of these
// publishers commit to a single canonical schema yet.
type upstreamEvent struct {
	UserID *string `json:"user_id,omitempty"`
	OrgID  *string `json:"org_id,omitempty"`
	// org-core's organization events sometimes use `organization_id`.
	OrganizationID *string `json:"organization_id,omitempty"`
}

// UpstreamInvalidator binds the subscribers + drives the cache-bust +
// republish loop.
type UpstreamInvalidator struct {
	client     *internalnats.Client
	cache      *redis.Client
	natsShared *internalnats.SharedPublisher
	subs       []*nats.Subscription
}

func NewUpstreamInvalidator(client *internalnats.Client, cache *redis.Client, natsShared *internalnats.SharedPublisher) *UpstreamInvalidator {
	return &UpstreamInvalidator{client: client, cache: cache, natsShared: natsShared}
}

// Start binds one queue subscriber per subject. Idempotent — calling
// twice replaces previous handlers via re-subscribe. A nil client or
// cache makes Start a no-op (useful for tests / off-mode dev).
func (u *UpstreamInvalidator) Start(ctx context.Context) error {
	if u == nil || u.client == nil || u.cache == nil {
		log.Warn().Msg("subscribers/upstream-invalidator: client or cache missing, skipping subscription")
		return nil
	}

	subjects := []string{
		SubjectUserProfileUpdated,
		SubjectOrgPlanChanged,
		SubjectOrgMemberAdded,
		SubjectOrgMemberRemoved,
		SubjectOrgUpdated,
		SubjectBillingAccountUpdated,
		SubjectBillingPlanChanged,
	}

	for _, subject := range subjects {
		s := subject // capture for closure
		// Queue group so multiple session-core replicas would share work
		// (only one replica today, but the queue name future-proofs it).
		sub, err := u.client.QueueSubscribe(s, "session-core-invalidator", u.handle(ctx, s))
		if err != nil {
			return err
		}
		u.subs = append(u.subs, sub)
		log.Info().Str("subject", s).Msg("subscribers/upstream-invalidator: subscribed")
	}
	return nil
}

// Stop drains the bound subscriptions. Safe to call multiple times.
func (u *UpstreamInvalidator) Stop() {
	if u == nil {
		return
	}
	for _, s := range u.subs {
		if err := s.Drain(); err != nil {
			log.Warn().Err(err).Msg("subscribers/upstream-invalidator: drain")
		}
	}
	u.subs = nil
}

func (u *UpstreamInvalidator) handle(ctx context.Context, subject string) nats.MsgHandler {
	return func(msg *nats.Msg) {
		var event upstreamEvent
		if err := json.Unmarshal(msg.Data, &event); err != nil {
			log.Warn().Err(err).Str("subject", subject).Msg("upstream-invalidator: bad payload")
			return
		}

		userID := derefOr(event.UserID, "")
		orgID := derefOr(event.OrgID, derefOr(event.OrganizationID, ""))

		// Bust the cache. When userID is known, wildcard-delete every org
		// snapshot for that user (the user-targeted events carry user_id;
		// org-targeted events without user_id need a broader sweep we
		// don't have today — see G34-followup-2 below).
		if userID != "" {
			if err := u.cache.InvalidateControlSession(ctx, userID, orgID); err != nil {
				log.Warn().Err(err).Str("subject", subject).Str("user_id", userID).Msg("upstream-invalidator: cache invalidate")
			}
		} else if orgID != "" {
			// Org-only event (e.g. org plan changed). G34-followup-2: use the
			// org->users reverse index (maintained on every snapshot cache
			// write) to bust exactly the affected users' snapshots
			// immediately, instead of waiting out the 30s TTL. Best-effort:
			// on a Redis error we log and fall back to the TTL bound (the
			// prior behavior); we never hard-fail or panic.
			n, err := u.cache.InvalidateOrgSessions(ctx, orgID)
			switch {
			case err != nil:
				log.Warn().Err(err).Str("subject", subject).Str("org_id", orgID).Msg("upstream-invalidator: org index invalidate degraded — relying on TTL")
			case n == 0:
				log.Debug().Str("subject", subject).Str("org_id", orgID).Msg("upstream-invalidator: org-only event, no indexed users — TTL backstops")
			default:
				log.Info().Str("subject", subject).Str("org_id", orgID).Int("invalidated", n).Msg("upstream-invalidator: org-only event, busted indexed user snapshots")
			}
		}

		// Re-publish so notification-core can fire the user-visible toast.
		// Only when we have a userID to scope the notification.
		if u.natsShared != nil && userID != "" {
			if err := u.natsShared.PublishAppSessionEntitlementsChanged(ctx, userID, orgID); err != nil {
				log.Warn().Err(err).Str("subject", subject).Msg("upstream-invalidator: republish degraded")
			}
		}
	}
}

func derefOr(p *string, fallback string) string {
	if p == nil {
		return fallback
	}
	return *p
}

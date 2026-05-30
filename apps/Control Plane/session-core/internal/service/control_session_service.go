package service

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/rs/zerolog/log"

	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/clients"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/convex"
	internalnats "github.com/I-Dacosta/CoreSystem/apps/session-core/internal/nats"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/redis"
)

// ControlSessionCacheTTL bounds the read-through cache lifetime (G34). 30s
// is the upper bound on staleness when no NATS-driven invalidation fires
// for an upstream change. Short enough that a manual `POST /refresh` is the
// fast path; long enough that dashboards hitting the endpoint per page nav
// don't fan out to 3 cores every time.
const ControlSessionCacheTTL = 30 * time.Second

// ControlSession is the aggregated app-context snapshot velion needs for
// post-login routing, plan gating, and entitlement-driven UI. It composes
// user-core (identity + onboarding routing), org-core (org + entitlements),
// and billing-core (subscription state) into one envelope.
//
// G10 (per ADR 0002): produced by the repurposed CP session-core, served at
// GET /api/v1/sessions/current.
type ControlSession struct {
	User             ControlSessionUser   `json:"user"`
	Organization     *ControlSessionOrg   `json:"organization,omitempty"`
	Entitlements     []clients.Entitlement `json:"entitlements"`
	Billing          *clients.BillingAccount `json:"billing,omitempty"`
	OnboardingStatus string               `json:"onboardingStatus"`
	FetchedAt        time.Time            `json:"fetchedAt"`
}

type ControlSessionUser struct {
	ID                 string `json:"id"`
	Email              string `json:"email,omitempty"`
	Name               string `json:"name,omitempty"`
	Image              string `json:"image,omitempty"`
	OnboardingComplete bool   `json:"onboardingComplete"`
}

type ControlSessionOrg struct {
	ID       string `json:"id"`
	Name     string `json:"name,omitempty"`
	Plan     string `json:"plan,omitempty"`
	TenantID string `json:"tenantId,omitempty"`
	Role     string `json:"role,omitempty"`
}

// ControlSessionService aggregates upstream cores into a single snapshot.
//
// G34 (cache): when `cache` is non-nil, `Get` reads through a per-user-and-
// org Redis entry with TTL `ControlSessionCacheTTL`. `Refresh` busts the
// entry before re-aggregating (so the post-refresh fetch goes upstream),
// then publishes `app.session.entitlements_changed`.
//
// G35 (Convex projection): when `convexClient` is non-nil, `Refresh` also
// mirrors the freshly-aggregated snapshot into Convex's `controlSessions`
// table so velion clients subscribed via `api.controlSessions.byUser` get
// reactive updates without polling.
type ControlSessionService struct {
	userClient    *clients.UserClient
	orgClient     *clients.OrgClient
	billingClient *clients.BillingClient
	natsShared    *internalnats.SharedPublisher
	cache         *redis.Client
	convexClient  *convex.Client
}

func NewControlSessionService(
	userClient *clients.UserClient,
	orgClient *clients.OrgClient,
	billingClient *clients.BillingClient,
	natsShared *internalnats.SharedPublisher,
	cache *redis.Client,
	convexClient *convex.Client,
) *ControlSessionService {
	return &ControlSessionService{
		userClient:    userClient,
		orgClient:     orgClient,
		billingClient: billingClient,
		natsShared:    natsShared,
		cache:         cache,
		convexClient:  convexClient,
	}
}

// ErrUserCoreUnavailable signals that the routing fields (org / role /
// onboardingStatus) could not be resolved. Callers should return 502.
var ErrUserCoreUnavailable = errors.New("control-session: user-core unavailable")

// Get assembles a Control Session for userID. Upstream calls happen in a
// best-effort fashion: user-core is required (failure → ErrUserCoreUnavailable);
// org-core + billing-core are degraded silently if they fail (the snapshot
// still returns with the user/onboarding fields populated).
//
// G34: read-through Redis cache. The first user-core call resolves the
// orgID; that key (`userID + orgID`) cached for 30s. Subsequent reads
// within the TTL skip the 3-core fan-out entirely.
func (s *ControlSessionService) Get(ctx context.Context, userID string) (*ControlSession, error) {
	if s.userClient == nil {
		return nil, ErrUserCoreUnavailable
	}

	routing, err := s.userClient.GetSessionContext(ctx, userID)
	if err != nil {
		return nil, fmt.Errorf("%w: %v", ErrUserCoreUnavailable, err)
	}

	// Cache lookup happens AFTER the cheap user-core routing call so we know
	// the orgID for the cache key. The routing fetch itself is single-RPC
	// and unlikely to dominate latency; the expensive fan-out (profile +
	// org + entitlements + billing) is what the cache shortcuts.
	if s.cache != nil {
		cached := new(ControlSession)
		if err := s.cache.GetCachedControlSession(ctx, userID, routing.OrgID, cached); err == nil {
			cached.FetchedAt = time.Now().UTC()
			return cached, nil
		} else if !errors.Is(err, redis.ErrCacheMiss) {
			log.Warn().Err(err).Str("user_id", userID).Msg("control-session: cache read degraded")
		}
	}

	out := &ControlSession{
		User:             ControlSessionUser{ID: routing.UserID},
		Entitlements:     []clients.Entitlement{},
		OnboardingStatus: routing.OnboardingStatus,
		FetchedAt:        time.Now().UTC(),
	}

	// Profile fields are non-fatal — degrade if user-core's /users/me trips.
	if profile, err := s.userClient.GetProfile(ctx, userID); err == nil && profile != nil {
		out.User.Email = profile.Email
		out.User.Name = profile.Name
		out.User.Image = profile.Image
		out.User.OnboardingComplete = profile.OnboardingComplete
	} else if err != nil {
		log.Warn().Err(err).Str("user_id", userID).Msg("control-session: profile fetch degraded")
	}

	if routing.OrgID != "" {
		out.Organization = &ControlSessionOrg{
			ID:   routing.OrgID,
			Role: routing.Role,
		}

		if s.orgClient != nil {
			if org, err := s.orgClient.GetOrganization(ctx, routing.OrgID); err == nil && org != nil {
				out.Organization.Name = org.Name
				out.Organization.Plan = org.Plan
				out.Organization.TenantID = org.TenantID
			} else if err != nil {
				log.Warn().Err(err).Str("org_id", routing.OrgID).Msg("control-session: org details degraded")
			}

			if ents, err := s.orgClient.GetEntitlements(ctx, routing.OrgID); err == nil && ents != nil {
				out.Entitlements = ents
			} else if err != nil {
				log.Warn().Err(err).Str("org_id", routing.OrgID).Msg("control-session: entitlements degraded")
			}
		}

		if s.billingClient != nil {
			if acct, err := s.billingClient.GetAccount(ctx, routing.OrgID); err == nil && acct != nil {
				out.Billing = acct
			} else if err != nil {
				log.Warn().Err(err).Str("org_id", routing.OrgID).Msg("control-session: billing degraded")
			}
		}
	}

	// G34: warm the cache for the next reader. Best-effort — cache failures
	// don't bubble to the caller.
	if s.cache != nil {
		if err := s.cache.CacheControlSession(ctx, userID, routing.OrgID, out, ControlSessionCacheTTL); err != nil {
			log.Warn().Err(err).Str("user_id", userID).Msg("control-session: cache write degraded")
		}
	}

	return out, nil
}

// Refresh re-aggregates and publishes app.session.entitlements_changed so
// notification-core (and any future subscribers) can react. G10 Step 3.
//
// Callers: velion's POST /api/v1/sessions/refresh proxy, used after explicit
// plan upgrades / org switches / billing webhook acks where the snapshot is
// known to be stale.
func (s *ControlSessionService) Refresh(ctx context.Context, userID string) (*ControlSession, error) {
	// G34: bust the cache before re-aggregating. Without this, an explicit
	// `/refresh` after a plan upgrade would still serve the stale snapshot
	// (within the 30s TTL window) and the publish below would broadcast the
	// PRE-change state to notification-core. Wildcard delete across all
	// orgIDs for this user so we don't need to know the org first.
	if s.cache != nil {
		if err := s.cache.InvalidateControlSession(ctx, userID, ""); err != nil {
			log.Warn().Err(err).Str("user_id", userID).Msg("control-session: cache invalidate degraded")
		}
	}

	snap, err := s.Get(ctx, userID)
	if err != nil {
		return nil, err
	}

	if s.natsShared != nil {
		if err := s.natsShared.PublishAppSessionEntitlementsChanged(ctx, snap.User.ID, orgIDOf(snap)); err != nil {
			log.Warn().Err(err).Str("user_id", userID).Msg("control-session: NATS publish degraded")
		}
	}

	// G35: mirror the snapshot into Convex so velion clients subscribed via
	// `api.controlSessions.byUser` get the update reactively. Best-effort;
	// Convex outages don't fail the refresh.
	if s.convexClient != nil {
		fetchedAtMillis := snap.FetchedAt.UnixMilli()
		if err := s.convexClient.MirrorControlSession(ctx, snap.User.ID, orgIDOf(snap), snap, fetchedAtMillis); err != nil {
			log.Warn().Err(err).Str("user_id", userID).Msg("control-session: convex mirror degraded")
		}
	}

	return snap, nil
}

func orgIDOf(s *ControlSession) string {
	if s == nil || s.Organization == nil {
		return ""
	}
	return s.Organization.ID
}

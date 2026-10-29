package billing

import (
	"context"
	"fmt"
	"log"
	"strings"
	"time"
)

// D-A billing-consolidation grant, inheritance half.
//
// Scope is deliberately narrow and was a product decision, not an engineering
// one: a member organization inherits the host's plan tier, and nothing else
// moves. Usage, invoices, and the payment-provider customer stay per-org --
// billing_accounts.org_id is still the billing unit. Consolidating actual
// invoicing would decide who is legally liable, how VAT applies when host and
// member differ, and how a mid-cycle join is prorated; none of those are
// settled, and charging the wrong entity is not a recoverable mistake.

// BillingGroup is the mirrored grant: which host organization an org inherits
// its plan from. Mirrored from auth-core via organization.billing_group.changed.
type BillingGroup struct {
	OrgID                string
	HostOrgID            string
	OrgGroupID           string
	BillingConsolidation bool
}

// planRank orders the plan tiers so inheritance can never DOWNGRADE a member
// organization that already pays for more than its host.
//
// The ordering is not invented: it follows normalizePlan's own switch order and
// is corroborated by defaultQuotaLimitsByPlan, where free/trial share the
// smallest quotas (1000 api_calls / 5 users), hobby/standard/pro share the
// middle tier, and enterprise is unlimited (-1). standard ranks below pro
// because pro additionally unlocks the lead-builder entitlement.
func planRank(plan string) int {
	switch normalizePlan(plan) {
	case "enterprise":
		return 4
	case "pro":
		return 3
	case "standard":
		return 2
	case "hobby":
		return 1
	default: // free, trial
		return 0
	}
}

// InheritedPlan resolves the plan an account should see once a billing-group
// grant is taken into account.
//
// `host` is nil when the org has no grant, when the grant carries no billing
// consolidation, or when the host's own account could not be loaded — every one
// of those falls back to the org's own effective plan, so a missing or stale
// mirror degrades to today's behaviour rather than to a wrong tier.
//
// Inheritance takes the MORE privileged of the two plans. Taking the host's plan
// unconditionally would silently downgrade a member that out-ranks its host,
// which is a paid-for regression rather than a grant.
func InheritedPlan(account Account, host *Account, now time.Time) string {
	own := EffectivePlan(account, now)
	if host == nil {
		return own
	}
	inherited := EffectivePlan(*host, now)
	if planRank(inherited) > planRank(own) {
		return inherited
	}
	return own
}

// ApplyBillingGroupChange mirrors an auth-core grant change. A revocation
// (billingConsolidation false, or no host) deletes the row so the org falls
// straight back to its own plan rather than keeping an inert relationship.
func (s *Service) ApplyBillingGroupChange(
	ctx context.Context,
	orgID, hostOrgID, orgGroupID string,
	billingConsolidation bool,
) error {
	if strings.TrimSpace(orgID) == "" {
		return fmt.Errorf("apply billing group change: orgID is required")
	}
	if !billingConsolidation || strings.TrimSpace(hostOrgID) == "" {
		return s.repo.DeleteBillingGroup(ctx, orgID)
	}
	if orgID == hostOrgID {
		// The CHECK constraint would reject it; refuse before the round trip so
		// the reason is legible in logs rather than surfacing as a DB error.
		return fmt.Errorf("apply billing group change: org cannot inherit from itself")
	}
	return s.repo.UpsertBillingGroup(ctx, BillingGroup{
		OrgID:                orgID,
		HostOrgID:            hostOrgID,
		OrgGroupID:           orgGroupID,
		BillingConsolidation: true,
	})
}

// ResolveEffectivePlan is EffectivePlan plus billing-group inheritance. Every
// failure path degrades to the org's own plan: a missing mirror, an unreadable
// host account, or a revoked grant must never leave an org on a tier it did not
// pay for, in either direction.
func (s *Service) ResolveEffectivePlan(
	ctx context.Context,
	account Account,
	now time.Time,
) string {
	group, err := s.repo.GetBillingGroup(ctx, account.OrgID)
	if err != nil || group == nil {
		if err != nil {
			log.Printf("billing-core billing-group lookup failed for %s: %v", account.OrgID, err)
		}
		return EffectivePlan(account, now)
	}
	host, err := s.repo.GetAccount(ctx, group.HostOrgID)
	if err != nil {
		log.Printf("billing-core host account lookup failed for %s: %v", group.HostOrgID, err)
		return EffectivePlan(account, now)
	}
	return InheritedPlan(account, &host, now)
}

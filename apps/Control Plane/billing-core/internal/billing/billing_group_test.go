package billing

import (
	"testing"
	"time"
)

func acct(orgID, plan string) Account {
	return Account{OrgID: orgID, Plan: plan, SubscriptionState: SubscriptionStateActive}
}

func TestInheritedPlanFallsBackToOwnPlanWithoutAHost(t *testing.T) {
	now := time.Now().UTC()
	if got := InheritedPlan(acct("b", "free"), nil, now); got != "free" {
		t.Fatalf("no host must fall back to the org's own plan, got %q", got)
	}
}

func TestInheritedPlanTakesTheHostsHigherTier(t *testing.T) {
	now := time.Now().UTC()
	host := acct("a", "enterprise")
	if got := InheritedPlan(acct("b", "free"), &host, now); got != "enterprise" {
		t.Fatalf("expected inheritance to enterprise, got %q", got)
	}
}

// The case that matters most: inheritance must never cost a member org a tier
// it already pays for.
func TestInheritedPlanNeverDowngradesAMemberBelowItsOwnTier(t *testing.T) {
	now := time.Now().UTC()
	host := acct("a", "free")
	if got := InheritedPlan(acct("b", "enterprise"), &host, now); got != "enterprise" {
		t.Fatalf("a lower-tier host must not downgrade the member, got %q", got)
	}
}

func TestInheritedPlanIsStableWhenTiersAreEqual(t *testing.T) {
	now := time.Now().UTC()
	host := acct("a", "pro")
	if got := InheritedPlan(acct("b", "pro"), &host, now); got != "pro" {
		t.Fatalf("expected pro, got %q", got)
	}
}

// A member mid-trial is elevated to TrialPlan by EffectivePlan; a lower-tier
// host must not pull it back down before the trial ends.
func TestInheritedPlanRespectsAnActiveTrialOnTheMember(t *testing.T) {
	now := time.Now().UTC()
	ends := now.Add(24 * time.Hour)
	member := Account{
		OrgID:             "b",
		Plan:              "free",
		SubscriptionState: SubscriptionStateTrialing,
		TrialEndsAt:       &ends,
	}
	host := acct("a", "hobby")
	// member's effective plan is TrialPlan ("pro", rank 3) vs host hobby (rank 1)
	if got := InheritedPlan(member, &host, now); got != TrialPlan {
		t.Fatalf("an active trial must not be downgraded by the host, got %q", got)
	}
}

func TestPlanRankOrderingMatchesTheQuotaTiers(t *testing.T) {
	if !(planRank("free") < planRank("hobby") &&
		planRank("hobby") < planRank("standard") &&
		planRank("standard") < planRank("pro") &&
		planRank("pro") < planRank("enterprise")) {
		t.Fatal("plan rank must be strictly ascending free<hobby<standard<pro<enterprise")
	}
	if planRank("trial") != planRank("free") {
		t.Fatal("trial ranks with free; the trial elevation is EffectivePlan's job")
	}
	// An unknown plan must not outrank anything real.
	if planRank("not-a-plan") != 0 {
		t.Fatal("unknown plans must rank at the bottom")
	}
}

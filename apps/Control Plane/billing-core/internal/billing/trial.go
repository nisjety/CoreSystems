package billing

import (
	"os"
	"strconv"
	"strings"
	"time"
)

const (
	// TrialPlan is the plan tier a new organization is granted during its
	// onboarding trial. The org's stored `plan` stays mirrored from org-core
	// (typically "free"); the trial elevates the *effective* plan + entitlements
	// so connector setup (integration-core requires "pro") works pre-paywall.
	TrialPlan = "pro"
	// DefaultTrialDays is the trial window when TRIAL_DURATION_DAYS is unset.
	DefaultTrialDays = 14
)

// trialDurationDays reads TRIAL_DURATION_DAYS (clamped to 1..90), default 14.
func trialDurationDays() int {
	raw := strings.TrimSpace(os.Getenv("TRIAL_DURATION_DAYS"))
	if raw == "" {
		return DefaultTrialDays
	}
	days, err := strconv.Atoi(raw)
	if err != nil || days < 1 {
		return DefaultTrialDays
	}
	if days > 90 {
		return 90
	}
	return days
}

// trialActive reports whether the account is in an unexpired trial window.
func trialActive(account Account, now time.Time) bool {
	return account.SubscriptionState == SubscriptionStateTrialing &&
		account.TrialEndsAt != nil &&
		now.Before(*account.TrialEndsAt)
}

// EffectivePlan is the plan a consumer (entitlement checks, integration-core
// plan gate) should see: the elevated trial plan during an active trial,
// otherwise the normalized stored plan.
func EffectivePlan(account Account, now time.Time) string {
	if trialActive(account, now) {
		return TrialPlan
	}
	return normalizePlan(account.Plan)
}

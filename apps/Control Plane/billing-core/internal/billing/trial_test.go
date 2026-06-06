package billing

import (
	"testing"
	"time"
)

func ptr(t time.Time) *time.Time { return &t }

func TestEffectivePlan(t *testing.T) {
	now := time.Date(2026, 1, 1, 12, 0, 0, 0, time.UTC)
	future := now.Add(24 * time.Hour)
	past := now.Add(-1 * time.Hour)

	cases := []struct {
		name    string
		account Account
		want    string
	}{
		{"active trial elevates to pro", Account{Plan: "free", SubscriptionState: SubscriptionStateTrialing, TrialEndsAt: ptr(future)}, "pro"},
		{"expired trial falls back to base plan", Account{Plan: "free", SubscriptionState: SubscriptionStateTrialing, TrialEndsAt: ptr(past)}, "free"},
		{"trialing without window is not a trial", Account{Plan: "free", SubscriptionState: SubscriptionStateTrialing}, "free"},
		{"active account uses normalized plan", Account{Plan: "essential", SubscriptionState: SubscriptionStateActive}, "hobby"},
		{"active paid plan unchanged", Account{Plan: "pro", SubscriptionState: SubscriptionStateActive}, "pro"},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := EffectivePlan(tc.account, now); got != tc.want {
				t.Fatalf("EffectivePlan = %q, want %q", got, tc.want)
			}
		})
	}
}

func TestTrialActive(t *testing.T) {
	now := time.Date(2026, 1, 1, 12, 0, 0, 0, time.UTC)
	if !trialActive(Account{SubscriptionState: SubscriptionStateTrialing, TrialEndsAt: ptr(now.Add(time.Hour))}, now) {
		t.Fatal("expected active trial")
	}
	if trialActive(Account{SubscriptionState: SubscriptionStateActive, TrialEndsAt: ptr(now.Add(time.Hour))}, now) {
		t.Fatal("active state is not a trial")
	}
	if trialActive(Account{SubscriptionState: SubscriptionStateTrialing, TrialEndsAt: ptr(now.Add(-time.Hour))}, now) {
		t.Fatal("expired window is not active")
	}
}

func TestTrialElevationGrantsIntegrations(t *testing.T) {
	// During the trial the effective plan is Pro, which must grant the
	// integrations entitlement that gates connector setup.
	if !defaultEntitlementsForPlan(TrialPlan)["feature.integrations"] {
		t.Fatal("trial (pro) plan must grant feature.integrations")
	}
	if defaultEntitlementsForPlan("free")["feature.integrations"] {
		t.Fatal("free plan must not grant feature.integrations")
	}
}

func TestTrialDurationDaysDefault(t *testing.T) {
	t.Setenv("TRIAL_DURATION_DAYS", "")
	if got := trialDurationDays(); got != DefaultTrialDays {
		t.Fatalf("default trial days = %d, want %d", got, DefaultTrialDays)
	}
	t.Setenv("TRIAL_DURATION_DAYS", "30")
	if got := trialDurationDays(); got != 30 {
		t.Fatalf("trial days = %d, want 30", got)
	}
	t.Setenv("TRIAL_DURATION_DAYS", "garbage")
	if got := trialDurationDays(); got != DefaultTrialDays {
		t.Fatalf("invalid trial days should fall back to %d, got %d", DefaultTrialDays, got)
	}
}

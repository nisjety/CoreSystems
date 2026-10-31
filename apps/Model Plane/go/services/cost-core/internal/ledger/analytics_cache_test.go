package ledger

import (
	"context"
	"testing"
	"time"
)

func cacheEntry(key string, basis string, input, read, creation int64, reason string) Entry {
	return Entry{
		OrgID: "org1", UserID: "u1", Model: "claude-sonnet", ProviderUsed: "anthropic",
		InputTokens: input, OutputTokens: 10, CostBasis: basis, IdempotencyKey: key,
		CacheReadInputTokens: read, CacheCreationInputTokens: creation, CacheMissReason: reason,
		CreatedAt: time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC),
	}
}

func TestNormalizeCacheMissReasonIsAClosedSet(t *testing.T) {
	for _, ok := range []string{"", "below_minimum", "prefix_changed", "ttl_expired", "no_baseline", "unexplained", " Prefix_Changed "} {
		if _, valid := NormalizeCacheMissReason(ok); !valid {
			t.Fatalf("%q must be accepted", ok)
		}
	}
	for _, bad := range []string{"cold", "miss", "prefix changed"} {
		if reason, valid := NormalizeCacheMissReason(bad); valid || reason != "" {
			t.Fatalf("%q must be rejected to empty, got %q/%v", bad, reason, valid)
		}
	}
	if err := ValidateEntry(cacheEntry("k", "metered", 100, 0, 0, "cold")); err == nil {
		t.Fatal("an entry with a reason outside the closed set must be invalid")
	}
	if err := ValidateEntry(cacheEntry("k", "metered", 100, 0, 0, "ttl_expired")); err != nil {
		t.Fatalf("a closed-set reason must validate: %v", err)
	}
}

func TestUsageAnalyticsSumsCacheLegsAndTheHitRateDenominator(t *testing.T) {
	ctx := context.Background()
	s := NewStore()
	mustRecord(t, s, cacheEntry("hit", "metered", 1000, 800, 0, ""))
	mustRecord(t, s, cacheEntry("write", "metered", 2000, 0, 1500, "no_baseline"))
	// Too short to cache: counted as usage, not as cache activity.
	mustRecord(t, s, cacheEntry("tiny", "metered", 300, 0, 0, "below_minimum"))
	// No reason and no legs: the provider reported nothing.
	mustRecord(t, s, cacheEntry("silent", "metered", 500, 0, 0, ""))
	// Subscription turns are excluded from every cache figure.
	mustRecord(t, s, cacheEntry("sub", "subscription", 4000, 3000, 0, ""))

	got, err := s.UsageAnalytics(ctx, AggregateFilter{OrgID: "org1"})
	if err != nil {
		t.Fatal(err)
	}
	var metered, subscription *UsageBucket
	for i := range got {
		switch got[i].Basis {
		case "metered":
			metered = &got[i]
		case "subscription":
			subscription = &got[i]
		}
	}
	if metered == nil || subscription == nil {
		t.Fatalf("expected a metered and a subscription bucket: %+v", got)
	}
	if metered.CacheReadInputTokens != 800 || metered.CacheCreationInputTokens != 1500 {
		t.Fatalf("metered cache legs = %d/%d, want 800/1500", metered.CacheReadInputTokens, metered.CacheCreationInputTokens)
	}
	if metered.CacheReportedInputTokens != 3000 {
		t.Fatalf("reported input = %d, want 3000 (hit + write only)", metered.CacheReportedInputTokens)
	}
	if subscription.CacheReadInputTokens != 0 || subscription.CacheReportedInputTokens != 0 {
		t.Fatalf("subscription must carry no cache figures: %+v", subscription)
	}
}

func TestCacheMissAnalyticsCountsReasonsAndSkipsSubscriptionAndUnclassified(t *testing.T) {
	ctx := context.Background()
	s := NewStore()
	mustRecord(t, s, cacheEntry("a", "metered", 2000, 0, 0, "prefix_changed"))
	mustRecord(t, s, cacheEntry("b", "metered", 2000, 0, 0, "prefix_changed"))
	mustRecord(t, s, cacheEntry("c", "", 2000, 0, 0, "ttl_expired"))
	mustRecord(t, s, cacheEntry("d", "metered", 2000, 1500, 0, ""))
	mustRecord(t, s, cacheEntry("e", "subscription", 2000, 0, 0, "no_baseline"))

	got, err := s.CacheMissAnalytics(ctx, AggregateFilter{OrgID: "org1"})
	if err != nil {
		t.Fatal(err)
	}
	counts := map[string]int64{}
	for _, b := range got {
		counts[b.Reason] += b.Requests
		if b.Day != "2026-10-01" || b.Model != "claude-sonnet" || b.Provider != "anthropic" {
			t.Fatalf("wrong bucket key: %+v", b)
		}
	}
	if len(counts) != 2 || counts["prefix_changed"] != 2 || counts["ttl_expired"] != 1 {
		t.Fatalf("reason counts = %v, want prefix_changed=2 ttl_expired=1", counts)
	}
}

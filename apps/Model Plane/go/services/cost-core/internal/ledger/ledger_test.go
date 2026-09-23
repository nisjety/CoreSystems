package ledger

import (
	"context"
	"errors"
	"math"
	"strings"
	"testing"
	"time"
)

func TestStore_RecordAndGetUsage(t *testing.T) {
	ctx := context.Background()
	s := NewStore()

	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u1", InputTokens: 100, OutputTokens: 50, CostUSD: 0.5})
	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u1", InputTokens: 10, OutputTokens: 5, CostUSD: 0.05})

	u, err := s.GetUsage(ctx, "org1", "u1")
	if err != nil {
		t.Fatalf("GetUsage: %v", err)
	}
	if u.TotalInputTokens != 110 || u.TotalOutputTokens != 55 {
		t.Fatalf("tokens = %d/%d, want 110/55", u.TotalInputTokens, u.TotalOutputTokens)
	}
	if u.TotalCostUSD != 0.55 {
		t.Fatalf("cost = %v, want 0.55", u.TotalCostUSD)
	}
	if u.EntryCount != 2 {
		t.Fatalf("entry count = %d, want 2", u.EntryCount)
	}
}

// Cache-token telemetry (native-compaction migration prerequisite): the
// in-memory rollup accumulates the two cache legs the same way it already
// accumulates InputTokens/OutputTokens.
func TestStore_RecordAndGetUsageRollsUpCacheTokens(t *testing.T) {
	ctx := context.Background()
	s := NewStore()

	mustRecord(t, s, Entry{
		OrgID: "org1", UserID: "u1", InputTokens: 8_520, OutputTokens: 42,
		CacheReadInputTokens: 8_000, CacheCreationInputTokens: 400,
	})
	mustRecord(t, s, Entry{
		OrgID: "org1", UserID: "u1", InputTokens: 20, OutputTokens: 5,
		CacheReadInputTokens: 15, CacheCreationInputTokens: 0,
	})

	u, err := s.GetUsage(ctx, "org1", "u1")
	if err != nil {
		t.Fatalf("GetUsage: %v", err)
	}
	if u.TotalCacheReadInputTokens != 8_015 || u.TotalCacheCreationInputTokens != 400 {
		t.Fatalf("cache token rollup = %d/%d, want 8015/400",
			u.TotalCacheReadInputTokens, u.TotalCacheCreationInputTokens)
	}
	// An entry that never mentions cache usage must roll up to exact zero,
	// not silently drop the whole entry.
	uncached := Entry{OrgID: "org2", UserID: "u9", InputTokens: 10, OutputTokens: 5}
	mustRecord(t, s, uncached)
	u2, err := s.GetUsage(ctx, "org2", "u9")
	if err != nil {
		t.Fatalf("GetUsage(org2): %v", err)
	}
	if u2.TotalCacheReadInputTokens != 0 || u2.TotalCacheCreationInputTokens != 0 {
		t.Fatalf("uncached rollup = %+v, want zero cache totals", u2)
	}
}

func TestStore_GetUsageNotFound(t *testing.T) {
	s := NewStore()
	_, err := s.GetUsage(context.Background(), "missing", "u")
	if !errors.Is(err, ErrUsageNotFound) {
		t.Fatalf("err = %v, want ErrUsageNotFound", err)
	}
}

func TestStore_Idempotency(t *testing.T) {
	ctx := context.Background()
	s := NewStore()
	e := Entry{OrgID: "org1", UserID: "u1", InputTokens: 7, IdempotencyKey: "dup-key"}
	mustRecord(t, s, e)
	mustRecord(t, s, e) // duplicate must be ignored

	u, err := s.GetUsage(ctx, "org1", "u1")
	if err != nil {
		t.Fatalf("GetUsage: %v", err)
	}
	if u.EntryCount != 1 || u.TotalInputTokens != 7 {
		t.Fatalf("dedupe failed: count=%d tokens=%d", u.EntryCount, u.TotalInputTokens)
	}
}

func TestStore_IdempotencyIsTenantScoped(t *testing.T) {
	ctx := context.Background()
	s := NewStore()
	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u1", InputTokens: 7, IdempotencyKey: "shared-key"})
	mustRecord(t, s, Entry{OrgID: "org2", UserID: "u2", InputTokens: 11, IdempotencyKey: "shared-key"})

	for _, test := range []struct {
		org, user string
		want      int64
	}{
		{org: "org1", user: "u1", want: 7},
		{org: "org2", user: "u2", want: 11},
	} {
		u, err := s.GetUsage(ctx, test.org, test.user)
		if err != nil {
			t.Fatalf("GetUsage(%s): %v", test.org, err)
		}
		if u.EntryCount != 1 || u.TotalInputTokens != test.want {
			t.Fatalf("usage(%s)=%+v want one entry/%d tokens", test.org, u, test.want)
		}
	}
}

// TestStore_PurgeOrgIsolatesOtherOrg is the mandatory GDPR safety test: purging
// one org's data must never affect another org's rows.
func TestStore_PurgeOrgIsolatesOtherOrg(t *testing.T) {
	ctx := context.Background()
	s := NewStore()
	mustRecord(t, s, Entry{OrgID: "org-a", UserID: "u1", RunID: "run1", InputTokens: 10, IdempotencyKey: "a-key"})
	mustRecord(t, s, Entry{OrgID: "org-b", UserID: "u2", RunID: "run1", InputTokens: 20, IdempotencyKey: "b-key"})

	if err := s.PurgeOrg(ctx, "org-a"); err != nil {
		t.Fatalf("PurgeOrg(org-a): %v", err)
	}

	if _, err := s.GetUsage(ctx, "org-a", "u1"); !errors.Is(err, ErrUsageNotFound) {
		t.Fatalf("org-a usage = %v, want ErrUsageNotFound after purge", err)
	}
	if entries, err := s.ListEntries(ctx, AggregateFilter{OrgID: "org-a"}, 10); err != nil || len(entries) != 0 {
		t.Fatalf("org-a entries = %v (err=%v), want none after purge", entries, err)
	}

	otherUsage, err := s.GetUsage(ctx, "org-b", "u2")
	if err != nil {
		t.Fatalf("org-b usage should survive purge of org-a: %v", err)
	}
	if otherUsage.TotalInputTokens != 20 || otherUsage.EntryCount != 1 {
		t.Fatalf("org-b usage corrupted by org-a purge: %+v", otherUsage)
	}
	otherEntries, err := s.ListEntries(ctx, AggregateFilter{OrgID: "org-b"}, 10)
	if err != nil || len(otherEntries) != 1 {
		t.Fatalf("org-b entries = %v (err=%v), want exactly 1 after org-a purge", otherEntries, err)
	}

	// Idempotent: purging again (redelivery) must not error or affect org-b.
	if err := s.PurgeOrg(ctx, "org-a"); err != nil {
		t.Fatalf("second PurgeOrg(org-a) should be a no-op, got: %v", err)
	}
	if _, err := s.GetUsage(ctx, "org-b", "u2"); err != nil {
		t.Fatalf("org-b usage should still be intact after redelivered purge: %v", err)
	}
}

func TestStoreRejectsMalformedAccountingEntries(t *testing.T) {
	for _, test := range []struct {
		name  string
		entry Entry
	}{
		{name: "missing attribution", entry: Entry{OrgID: "org1"}},
		{name: "negative input", entry: Entry{OrgID: "org1", UserID: "u1", InputTokens: -1}},
		{name: "negative output", entry: Entry{OrgID: "org1", UserID: "u1", OutputTokens: -1}},
		{name: "excessive input", entry: Entry{OrgID: "org1", UserID: "u1", InputTokens: 1_000_000_000_001}},
		{name: "negative cost", entry: Entry{OrgID: "org1", UserID: "u1", CostUSD: -0.01}},
		{name: "nan cost", entry: Entry{OrgID: "org1", UserID: "u1", CostUSD: math.NaN()}},
		{name: "infinite cost", entry: Entry{OrgID: "org1", UserID: "u1", CostUSD: math.Inf(1)}},
		{name: "excessive cost", entry: Entry{OrgID: "org1", UserID: "u1", CostUSD: 10_000_000_000}},
		{name: "non canonical organization", entry: Entry{OrgID: " org1", UserID: "u1"}},
		{name: "control character", entry: Entry{OrgID: "org1", UserID: "u1\nforged"}},
		{name: "oversized model", entry: Entry{OrgID: "org1", UserID: "u1", Model: strings.Repeat("m", 257)}},
		{name: "oversized idempotency", entry: Entry{OrgID: "org1", UserID: "u1", IdempotencyKey: strings.Repeat("k", 513)}},
	} {
		t.Run(test.name, func(t *testing.T) {
			if err := NewStore().RecordEntry(context.Background(), test.entry); err == nil {
				t.Fatal("expected invalid entry to be rejected")
			}
		})
	}
}

func TestStore_GetRunUsage(t *testing.T) {
	ctx := context.Background()
	s := NewStore()
	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u1", RunID: "run-A", InputTokens: 10, CostUSD: 1})
	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u2", RunID: "run-A", OutputTokens: 20, CostUSD: 2})
	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u1", RunID: "run-B", InputTokens: 99})

	u, err := s.GetRunUsage(ctx, "run-A")
	if err != nil {
		t.Fatalf("GetRunUsage: %v", err)
	}
	if u.TotalInputTokens != 10 || u.TotalOutputTokens != 20 || u.TotalCostUSD != 3 || u.EntryCount != 2 {
		t.Fatalf("run-A rollup wrong: %+v", u)
	}

	if _, err := s.GetRunUsage(ctx, "no-such-run"); !errors.Is(err, ErrUsageNotFound) {
		t.Fatalf("missing run err = %v, want ErrUsageNotFound", err)
	}
}

func TestStore_AggregateWithFilters(t *testing.T) {
	ctx := context.Background()
	s := NewStore()
	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u1", Model: "gpt", InputTokens: 1, CostUSD: 0.1})
	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u2", Model: "gpt", InputTokens: 2, CostUSD: 0.2})
	mustRecord(t, s, Entry{OrgID: "org2", UserID: "u3", Model: "claude", InputTokens: 4, CostUSD: 0.4})

	// Org-wide rollup.
	u, _ := s.Aggregate(ctx, AggregateFilter{OrgID: "org1"})
	if u.TotalInputTokens != 3 || u.EntryCount != 2 {
		t.Fatalf("org1 aggregate wrong: %+v", u)
	}

	// Model-scoped rollup across orgs.
	u, _ = s.Aggregate(ctx, AggregateFilter{Model: "gpt"})
	if u.EntryCount != 2 || u.TotalInputTokens != 3 || u.TotalCostUSD < 0.29 || u.TotalCostUSD > 0.31 {
		t.Fatalf("gpt aggregate wrong: %+v", u)
	}

	// No match yields zero-valued Usage, not an error.
	u, err := s.Aggregate(ctx, AggregateFilter{OrgID: "nope"})
	if err != nil {
		t.Fatalf("Aggregate err = %v", err)
	}
	if u.EntryCount != 0 {
		t.Fatalf("empty aggregate count = %d, want 0", u.EntryCount)
	}
}

func TestStore_ListEntriesNewestFirst(t *testing.T) {
	ctx := context.Background()
	s := NewStore()
	base := time.Now().UTC()
	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u1", Model: "old", CreatedAt: base.Add(-2 * time.Hour)})
	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u1", Model: "new", CreatedAt: base})
	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u1", Model: "mid", CreatedAt: base.Add(-1 * time.Hour)})

	entries, err := s.ListEntries(ctx, AggregateFilter{OrgID: "org1"}, 2)
	if err != nil {
		t.Fatalf("ListEntries: %v", err)
	}
	if len(entries) != 2 {
		t.Fatalf("got %d entries, want 2 (limit)", len(entries))
	}
	if entries[0].Model != "new" || entries[1].Model != "mid" {
		t.Fatalf("ordering wrong: %s, %s", entries[0].Model, entries[1].Model)
	}
}

func TestStore_CheckBudget(t *testing.T) {
	ctx := context.Background()
	s := NewStore()
	mustRecord(t, s, Entry{OrgID: "org1", UserID: "u1", InputTokens: 600, OutputTokens: 500, CostUSD: 5})

	// Within budget.
	if err := s.CheckBudget(ctx, "org1", "u1", 10, 2000); err != nil {
		t.Fatalf("within budget err = %v", err)
	}
	// Cost cap exceeded.
	if err := s.CheckBudget(ctx, "org1", "u1", 5, 0); !errors.Is(err, ErrBudgetExceededCost) {
		t.Fatalf("cost cap err = %v, want ErrBudgetExceededCost", err)
	}
	// Token cap exceeded (1100 >= 1000).
	if err := s.CheckBudget(ctx, "org1", "u1", 0, 1000); !errors.Is(err, ErrBudgetExceededTokens) {
		t.Fatalf("token cap err = %v, want ErrBudgetExceededTokens", err)
	}
	// Unknown key is always within budget.
	if err := s.CheckBudget(ctx, "org1", "ghost", 0.0001, 1); err != nil {
		t.Fatalf("unknown key err = %v, want nil", err)
	}
}

func mustRecord(t *testing.T, s *Store, e Entry) {
	t.Helper()
	if err := s.RecordEntry(context.Background(), e); err != nil {
		t.Fatalf("RecordEntry: %v", err)
	}
}

package ledger

import (
	"context"
	"fmt"
	"testing"
	"time"
)

func TestUserUsageIncludesEntireWindowAndSeparatesTenants(t *testing.T) {
	store := NewStore()
	now := time.Now().UTC()
	for i := 0; i < 151; i++ {
		err := store.RecordEntry(context.Background(), Entry{OrgID: "org-a", UserID: "user-a", InputTokens: 10, OutputTokens: 2, CostBasis: CostBasisMetered, CostUSD: 0.01, CreatedAt: now, IdempotencyKey: fmt.Sprintf("%d", i)})
		if err != nil {
			t.Fatal(err)
		}
	}
	for _, e := range []Entry{
		{OrgID: "org-b", UserID: "user-a", InputTokens: 900, CreatedAt: now},
		{OrgID: "org-a", UserID: "user-a", InputTokens: 500, CreatedAt: now.Add(-2 * time.Hour)},
		{OrgID: "org-a", UserID: "user-b", InputTokens: 30, CostBasis: CostBasisSubscription, CreatedAt: now},
	} {
		if err := store.RecordEntry(context.Background(), e); err != nil {
			t.Fatal(err)
		}
	}
	rows, err := store.AggregateUsers(context.Background(), AggregateFilter{OrgID: "org-a", Since: now.Add(-time.Hour), Until: now.Add(time.Hour)})
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 2 || rows[0].EntryCount != 151 || rows[0].InputTokens != 1510 || rows[1].SubscriptionEntries != 1 {
		t.Fatalf("rows=%#v", rows)
	}
}

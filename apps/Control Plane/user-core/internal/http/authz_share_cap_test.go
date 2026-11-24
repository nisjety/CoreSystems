package http

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// F-t1-057: the document share cap is enforced by Control in the same step as
// the insert, so concurrent sharers cannot overshoot it.

func TestOwnerAssertedUserGrantCapIsTwoHundredForDocumentsOnly(t *testing.T) {
	if got := ownerAssertedUserGrantCap("document"); got != 200 {
		t.Fatalf("document cap = %d, want 200", got)
	}
	// A snapshot (and any other type) keeps its own rules: no Control cap here.
	for _, rt := range []string{conversationSnapshotType, "thread", ""} {
		if got := ownerAssertedUserGrantCap(rt); got != 0 {
			t.Fatalf("%q cap = %d, want 0", rt, got)
		}
	}
}

// Live (TEST_DATABASE_URL): racing writers for new subjects on one document end
// with exactly cap user rows; the rest get ErrShareLimitReached, and a re-share
// of an existing subject at the cap still succeeds.
func TestGrantCappedCountsAndInsertsInOneStepUnderConcurrencyLive(t *testing.T) {
	pool := d12Pool(t)
	ctx := context.Background()
	repo := users.NewAclRepository(&database.DB{Pool: pool})
	const org, doc, capN, writers = "ft1057-org", "ft1057-doc", 3, 12
	t.Cleanup(func() { _, _ = pool.Exec(ctx, `DELETE FROM resource_grants WHERE org_id = $1`, org) })
	_, _ = pool.Exec(ctx, `DELETE FROM resource_grants WHERE org_id = $1`, org)

	var wg sync.WaitGroup
	var mu sync.Mutex
	granted, limited := 0, 0
	for i := 0; i < writers; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			_, err := repo.GrantCapped(ctx, &users.ResourceGrant{
				OrgID: org, ResourceType: "document", ResourceID: doc,
				SubjectType: "user", SubjectID: fmt.Sprintf("ft1057-user-%d", i), Role: "view", GrantedBy: "owner",
			}, capN)
			mu.Lock()
			defer mu.Unlock()
			switch {
			case err == nil:
				granted++
			case errors.Is(err, users.ErrShareLimitReached):
				limited++
			default:
				t.Errorf("writer %d: %v", i, err)
			}
		}(i)
	}
	wg.Wait()
	var rows int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM resource_grants WHERE org_id=$1 AND resource_id=$2 AND subject_type='user'`, org, doc).Scan(&rows); err != nil {
		t.Fatalf("count: %v", err)
	}
	if granted != capN || limited != writers-capN || rows != capN {
		t.Fatalf("granted=%d limited=%d rows=%d, want %d/%d/%d", granted, limited, rows, capN, writers-capN, capN)
	}

	var existing string
	if err := pool.QueryRow(ctx, `SELECT subject_id FROM resource_grants WHERE org_id=$1 AND resource_id=$2 LIMIT 1`, org, doc).Scan(&existing); err != nil {
		t.Fatalf("pick existing: %v", err)
	}
	if _, err := repo.GrantCapped(ctx, &users.ResourceGrant{
		OrgID: org, ResourceType: "document", ResourceID: doc,
		SubjectType: "user", SubjectID: existing, Role: "view", GrantedBy: "owner",
	}, capN); err != nil {
		t.Fatalf("re-share of an existing subject at the cap: %v", err)
	}
}

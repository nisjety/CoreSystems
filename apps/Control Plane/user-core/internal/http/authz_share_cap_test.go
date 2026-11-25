package http

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"

	"github.com/gin-gonic/gin"

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

	// The writers all start together, so a split count-then-insert would race.
	start := make(chan struct{})
	var wg sync.WaitGroup
	var mu sync.Mutex
	granted, limited := 0, 0
	for i := 0; i < writers; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			<-start
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
	close(start)
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

func TestWriteGrantWriteErrorMapsTheCapToATyped409(t *testing.T) {
	gin.SetMode(gin.TestMode)
	for _, tc := range []struct {
		err      error
		status   int
		wantCode string
	}{
		{fmt.Errorf("wrapped: %w", users.ErrShareLimitReached), http.StatusConflict, "share_limit_reached"},
		{errors.New("grant rejected: subject_type not supported"), http.StatusBadRequest, ""},
	} {
		rec := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(rec)
		writeGrantWriteError(c, tc.err)
		var body map[string]any
		if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
			t.Fatalf("decode: %v", err)
		}
		code, _ := body["code"].(string)
		if rec.Code != tc.status || code != tc.wantCode {
			t.Fatalf("%v: %d %v, want %d code %q", tc.err, rec.Code, body, tc.status, tc.wantCode)
		}
	}
}

func TestOwnerAssertedDocumentCapMatchesTheRepositoryCap(t *testing.T) {
	// documents-api's maxSharesPerDocument mirrors this number (F-t1-057).
	if ownerAssertedDocumentShareCap != users.DocumentUserGrantCap || users.DocumentUserGrantCap != 200 {
		t.Fatalf("caps drifted: facade %d, repository %d", ownerAssertedDocumentShareCap, users.DocumentUserGrantCap)
	}
}

// Live: Space subject rows on a document never count toward its user cap.
func TestGrantCappedIgnoresSpaceRowsLive(t *testing.T) {
	pool := d12Pool(t)
	ctx := context.Background()
	repo := users.NewAclRepository(&database.DB{Pool: pool})
	const org, doc = "ft1057-space-org", "ft1057-space-doc"
	t.Cleanup(func() { _, _ = pool.Exec(ctx, `DELETE FROM resource_grants WHERE org_id = $1`, org) })
	_, _ = pool.Exec(ctx, `DELETE FROM resource_grants WHERE org_id = $1`, org)
	for i := 0; i < 3; i++ {
		if _, err := pool.Exec(ctx, `
			INSERT INTO resource_grants (grant_id, org_id, resource_type, resource_id, subject_type, subject_id, role, granted_by)
			VALUES ($1,$2,'document',$3,'space',$4,'view','owner')`,
			fmt.Sprintf("ft1057-space-grant-%d", i), org, doc, fmt.Sprintf("ft1057-space-%d", i)); err != nil {
			t.Fatalf("seed space row: %v", err)
		}
	}
	for i := 0; i < 2; i++ {
		if _, err := repo.GrantCapped(ctx, &users.ResourceGrant{
			OrgID: org, ResourceType: "document", ResourceID: doc,
			SubjectType: "user", SubjectID: fmt.Sprintf("ft1057-space-user-%d", i), Role: "view", GrantedBy: "owner",
		}, 2); err != nil {
			t.Fatalf("user grant %d under a cap of 2 with 3 Space rows: %v", i, err)
		}
	}
	_, err := repo.GrantCapped(ctx, &users.ResourceGrant{
		OrgID: org, ResourceType: "document", ResourceID: doc,
		SubjectType: "user", SubjectID: "ft1057-space-user-extra", Role: "view", GrantedBy: "owner",
	}, 2)
	if !errors.Is(err, users.ErrShareLimitReached) {
		t.Fatalf("third user grant: %v, want ErrShareLimitReached", err)
	}
}

// Live: the org's Control ZDR attestation is what the owner-asserted grant
// path refuses on; no policy row is no attestation.
func TestOrgZeroDataRetentionReadsTheEffectPolicyLive(t *testing.T) {
	pool := d12Pool(t)
	ctx := context.Background()
	acl := users.NewAclRepository(&database.DB{Pool: pool})
	_, _ = pool.Exec(ctx, `DELETE FROM space_effect_policies WHERE org_id = 'ft1057-no-policy-org'`)
	if zdr, err := acl.OrgZeroDataRetention(ctx, "ft1057-no-policy-org"); err != nil || zdr {
		t.Fatalf("no policy row: zdr=%v err=%v", zdr, err)
	}
	d12Seed(t, pool, newLiveSpaceRepo(pool), true)
	if zdr, err := acl.OrgZeroDataRetention(ctx, d12Org); err != nil || zdr {
		t.Fatalf("non-ZDR policy: zdr=%v err=%v", zdr, err)
	}
	if _, err := pool.Exec(ctx, `UPDATE space_effect_policies SET zero_data_retention = TRUE WHERE org_id = $1`, d12Org); err != nil {
		t.Fatalf("set ZDR: %v", err)
	}
	if zdr, err := acl.OrgZeroDataRetention(ctx, d12Org); err != nil || !zdr {
		t.Fatalf("ZDR policy: zdr=%v err=%v", zdr, err)
	}
}

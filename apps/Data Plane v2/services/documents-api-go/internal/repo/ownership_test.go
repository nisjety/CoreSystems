//go:build integration

package repo_test

// Per-User Data Ownership & Sharing — filter-at-source tests (PR-2).
//
// These run against a Postgres pointed to by TEST_DATABASE_URL (a throwaway DB);
// they are skipped when it is unset. Each test isolates itself with a unique
// org_id and cleans up after itself, so they are safe against a shared DB.

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
)

func setupOwnershipDB(t *testing.T) (*repo.DocumentRepo, string, func()) {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping documents ownership integration test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	if _, err := pool.Exec(ctx, schemaSQL); err != nil {
		pool.Close()
		t.Fatalf("ensure schema: %v", err)
	}
	// Every DocumentRepo call below traverses orgscope.WithOrgScope, so the
	// scoped runtime role must exist and hold grants on this database's tables.
	// See grantScopedRuntimeRole (integration_test.go) for why a test fixture
	// needs a database role at all.
	grantScopedRuntimeRole(ctx, t, pool)
	org := fmt.Sprintf("owntest-%d-%d", os.Getpid(), time.Now().UnixNano())
	cleanup := func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM documents WHERE org_id=$1`, org)
		pool.Close()
	}
	return repo.NewDocumentRepo(pool), org, cleanup
}

func createDoc(t *testing.T, r *repo.DocumentRepo, org, title, owner, visibility string) string {
	t.Helper()
	res, err := r.Create(context.Background(), model.CreateDocumentInput{
		OrgID: org, Source: "s", Type: "note", Title: title, Content: "x",
		OwnerID: owner, Visibility: visibility,
	})
	if err != nil {
		t.Fatalf("create %q: %v", title, err)
	}
	return res.Document.DocumentID
}

func listIDs(t *testing.T, r *repo.DocumentRepo, in model.ListDocumentsInput) map[string]bool {
	t.Helper()
	res, err := r.List(context.Background(), in)
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	ids := make(map[string]bool, len(res.Documents))
	for _, d := range res.Documents {
		ids[d.DocumentID] = true
	}
	return ids
}

// A doc marked private by A must be ABSENT from B's List and Get; A still sees it.
func TestOwnershipPrivateHiddenFromNonOwner(t *testing.T) {
	r, org, cleanup := setupOwnershipDB(t)
	defer cleanup()
	ctx := context.Background()

	priv := createDoc(t, r, org, "A private", "user-a", "private")

	// user-b (no grant) cannot see it via List...
	if listIDs(t, r, model.ListDocumentsInput{OrgID: org, ViewerID: "user-b", Limit: 100})[priv] {
		t.Fatal("private doc leaked into non-owner's List")
	}
	// ...nor via Get (filtered → ErrNoRows).
	if _, err := r.Get(ctx, org, priv, "user-b", nil); err == nil {
		t.Fatal("private doc returned to non-owner via Get")
	}
	// Owner A sees it.
	if !listIDs(t, r, model.ListDocumentsInput{OrgID: org, ViewerID: "user-a", Limit: 100})[priv] {
		t.Fatal("owner cannot see own private doc")
	}
	if _, err := r.Get(ctx, org, priv, "user-a", nil); err != nil {
		t.Fatalf("owner Get on own private doc failed: %v", err)
	}
}

// An org-visible doc is seen by every org member.
func TestOwnershipOrgVisibleToAll(t *testing.T) {
	r, org, cleanup := setupOwnershipDB(t)
	defer cleanup()

	orgDoc := createDoc(t, r, org, "Org doc", "user-a", "org")
	for _, viewer := range []string{"user-a", "user-b", "user-c"} {
		if !listIDs(t, r, model.ListDocumentsInput{OrgID: org, ViewerID: viewer, Limit: 100})[orgDoc] {
			t.Fatalf("org-visible doc not seen by %s", viewer)
		}
	}
}

// A per-user grant scopes visibility to EXACTLY that user + that doc.
func TestOwnershipGrantScopesToUserAndDoc(t *testing.T) {
	r, org, cleanup := setupOwnershipDB(t)
	defer cleanup()

	priv := createDoc(t, r, org, "A private shared to B", "user-a", "private")
	other := createDoc(t, r, org, "A other private", "user-a", "private")

	// Simulate the resource_grant for (priv → user-b): documents-api receives
	// the granted id set from user-core's ListVisible facade.
	grantToB := []string{priv}

	bWithGrant := listIDs(t, r, model.ListDocumentsInput{OrgID: org, ViewerID: "user-b", GrantedIDs: grantToB, Limit: 100})
	if !bWithGrant[priv] {
		t.Fatal("granted doc not visible to grantee")
	}
	if bWithGrant[other] {
		t.Fatal("grant leaked a non-granted private doc to the grantee")
	}
	// user-c (no grant) sees neither private doc.
	cNoGrant := listIDs(t, r, model.ListDocumentsInput{OrgID: org, ViewerID: "user-c", Limit: 100})
	if cNoGrant[priv] || cNoGrant[other] {
		t.Fatal("private docs leaked to an ungranted user")
	}
	// Get honours the grant for B but not for C.
	if _, err := r.Get(context.Background(), org, priv, "user-b", grantToB); err != nil {
		t.Fatalf("grantee Get failed: %v", err)
	}
	if _, err := r.Get(context.Background(), org, priv, "user-c", nil); err == nil {
		t.Fatal("ungranted user got the private doc via Get")
	}
}

// GDPR erasure: transferring an erased user's owned docs to the system account
// removes their private docs from their own view (they no longer own them) while
// org-visible docs remain visible to everyone.
func TestTransferOwnershipOnErasure(t *testing.T) {
	r, org, cleanup := setupOwnershipDB(t)
	defer cleanup()
	ctx := context.Background()

	priv := createDoc(t, r, org, "A private", "user-a", "private")
	orgDoc := createDoc(t, r, org, "A org", "user-a", "org")

	// Before erasure user-a sees both (owner of priv + org-visible).
	before := listIDs(t, r, model.ListDocumentsInput{OrgID: org, ViewerID: "user-a", Limit: 100})
	if !before[priv] || !before[orgDoc] {
		t.Fatalf("pre-transfer user-a should see both docs, got %v", before)
	}

	n, err := r.TransferOwnership(ctx, org, "user-a", "org-system-account")
	if err != nil || n != 2 {
		t.Fatalf("TransferOwnership = (%d, %v); want (2, nil)", n, err)
	}

	// After transfer the private doc is owned by the system account, so user-a
	// (no longer owner, no grant) cannot see it; the org doc stays visible.
	after := listIDs(t, r, model.ListDocumentsInput{OrgID: org, ViewerID: "user-a", Limit: 100})
	if after[priv] {
		t.Fatal("after erasure-transfer, the erased user's private doc must not be visible to them")
	}
	if !after[orgDoc] {
		t.Fatal("org-visible doc should remain visible after transfer")
	}

	// Idempotent: a re-delivery transfers nothing.
	if n2, _ := r.TransferOwnership(ctx, org, "user-a", "org-system-account"); n2 != 0 {
		t.Fatalf("re-transfer should move 0 rows, moved %d", n2)
	}
}

// Back-compat: a request with no viewer (empty user_id) sees everything in the
// org, including private docs — the legacy org-scoped path is preserved.
func TestOwnershipNoViewerSeesAllLegacy(t *testing.T) {
	r, org, cleanup := setupOwnershipDB(t)
	defer cleanup()

	priv := createDoc(t, r, org, "private", "user-a", "private")
	orgDoc := createDoc(t, r, org, "org", "user-a", "org")

	all := listIDs(t, r, model.ListDocumentsInput{OrgID: org, ViewerID: "", Limit: 100})
	if !all[priv] || !all[orgDoc] {
		t.Fatal("legacy no-viewer path should see all org docs incl private")
	}
}

// createDocWithSource is like createDoc but lets the caller set a distinct
// `source` so the facet's per-source grouping can be exercised.
func createDocWithSource(t *testing.T, r *repo.DocumentRepo, org, title, source, owner, visibility string) string {
	t.Helper()
	res, err := r.Create(context.Background(), model.CreateDocumentInput{
		OrgID: org, Source: source, Type: "note", Title: title, Content: "x",
		OwnerID: owner, Visibility: visibility,
	})
	if err != nil {
		t.Fatalf("create %q: %v", title, err)
	}
	return res.Document.DocumentID
}

func facetSources(t *testing.T, r *repo.DocumentRepo, org, viewer string, granted []string) map[string]int {
	t.Helper()
	rows, _, err := r.SourcesFacet(context.Background(), org, viewer, granted)
	if err != nil {
		t.Fatalf("facet: %v", err)
	}
	out := make(map[string]int, len(rows))
	for _, sc := range rows {
		out[sc.Source] = sc.DocumentCount
	}
	return out
}

// The Sources facet must NOT count documents the viewer cannot read — it must
// apply the identical owner/org/grant predicate as List/Get. Before the fix it
// returned org-wide counts, leaking the existence + count of others' private docs.
func TestSourcesFacetRespectsOwnership(t *testing.T) {
	r, org, cleanup := setupOwnershipDB(t)
	defer cleanup()

	createDocWithSource(t, r, org, "a-private", "src-a", "user-a", "private")
	createDocWithSource(t, r, org, "b-org", "src-b", "user-b", "org")
	createDocWithSource(t, r, org, "c-private-b", "src-c", "user-b", "private")

	// user-a: own private (src-a) + org-visible (src-b); NOT user-b's private src-c.
	a := facetSources(t, r, org, "user-a", nil)
	if _, ok := a["src-a"]; !ok {
		t.Error("user-a must see own private source src-a")
	}
	if _, ok := a["src-b"]; !ok {
		t.Error("user-a must see org-visible source src-b")
	}
	if _, ok := a["src-c"]; ok {
		t.Error("LEAK: user-a must NOT see user-b's private source src-c")
	}

	// user-b: org-visible (src-b) + own private (src-c); NOT user-a's private src-a.
	b := facetSources(t, r, org, "user-b", nil)
	if _, ok := b["src-c"]; !ok {
		t.Error("user-b must see own private source src-c")
	}
	if _, ok := b["src-a"]; ok {
		t.Error("LEAK: user-b must NOT see user-a's private source src-a")
	}

	// Legacy no-viewer path still sees everything (back-compat).
	legacy := facetSources(t, r, org, "", nil)
	for _, want := range []string{"src-a", "src-b", "src-c"} {
		if _, ok := legacy[want]; !ok {
			t.Errorf("legacy no-viewer facet should include %s", want)
		}
	}
}

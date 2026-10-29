package users_test

import (
	"context"
	"os"
	"testing"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// resourceGrantsDDL mirrors migration 012 so the integration test is
// self-contained against any Postgres pointed to by TEST_DATABASE_URL.
const resourceGrantsDDL = `
CREATE TABLE IF NOT EXISTS resource_grants (
    grant_id      TEXT PRIMARY KEY,
    org_id        TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id   TEXT NOT NULL,
    subject_type  TEXT NOT NULL,
    subject_id    TEXT NOT NULL,
    role          TEXT NOT NULL,
    granted_by    TEXT NOT NULL DEFAULT '',
    granted_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT resource_grants_subject_type_chk CHECK (subject_type IN ('user', 'team')),
    CONSTRAINT resource_grants_role_chk         CHECK (role IN ('view', 'edit')),
    CONSTRAINT resource_grants_unique           UNIQUE (org_id, resource_type, resource_id, subject_type, subject_id)
);`

func newTestAclRepo(t *testing.T) (*users.AclRepository, string, func()) {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping resource_grants integration test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	if _, err := pool.Exec(ctx, resourceGrantsDDL); err != nil {
		pool.Close()
		t.Fatalf("ensure resource_grants: %v", err)
	}
	db := &database.DB{Pool: pool}
	repo := users.NewAclRepository(db)
	org := "owntest-" + uuid.NewString() // isolate this run from any other data
	cleanup := func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM resource_grants WHERE org_id=$1`, org)
		pool.Close()
	}
	return repo, org, cleanup
}

func TestGrantCheckRevoke(t *testing.T) {
	repo, org, cleanup := newTestAclRepo(t)
	defer cleanup()
	ctx := context.Background()

	const (
		doc  = "doc-1"
		user = "user-a"
	)

	// No grant yet → Check is false.
	if ok, _, err := repo.Check(ctx, org, "document", doc, "user", user); err != nil || ok {
		t.Fatalf("pre-grant Check = (%v, err=%v); want (false, nil)", ok, err)
	}

	g, err := repo.Grant(ctx, &users.ResourceGrant{
		OrgID: org, ResourceType: "document", ResourceID: doc,
		SubjectType: "user", SubjectID: user, Role: "view", GrantedBy: "owner-x",
	})
	if err != nil {
		t.Fatalf("Grant: %v", err)
	}
	if g.Role != "view" {
		t.Fatalf("granted role = %q, want view", g.Role)
	}

	ok, role, err := repo.Check(ctx, org, "document", doc, "user", user)
	if err != nil || !ok || role != "view" {
		t.Fatalf("post-grant Check = (%v, %q, err=%v); want (true, view, nil)", ok, role, err)
	}

	// Re-grant with a different role is an idempotent upsert (no duplicate row).
	if _, err := repo.Grant(ctx, &users.ResourceGrant{
		OrgID: org, ResourceType: "document", ResourceID: doc,
		SubjectType: "user", SubjectID: user, Role: "edit", GrantedBy: "owner-x",
	}); err != nil {
		t.Fatalf("re-Grant: %v", err)
	}
	grants, err := repo.ListByResource(ctx, org, "document", doc)
	if err != nil {
		t.Fatalf("ListByResource: %v", err)
	}
	if len(grants) != 1 || grants[0].Role != "edit" {
		t.Fatalf("after upsert want exactly 1 grant role=edit, got %d %+v", len(grants), grants)
	}

	// Revoke removes it.
	if err := repo.Revoke(ctx, org, "document", doc, "user", user); err != nil {
		t.Fatalf("Revoke: %v", err)
	}
	if ok, _, _ := repo.Check(ctx, org, "document", doc, "user", user); ok {
		t.Fatal("post-revoke Check should be false")
	}
}

func TestListVisibleAndBatchCheckScopePerUser(t *testing.T) {
	repo, org, cleanup := newTestAclRepo(t)
	defer cleanup()
	ctx := context.Background()

	// user-a granted doc-1 + doc-2; user-b granted doc-3 only.
	mustGrant(t, repo, org, "doc-1", "user-a")
	mustGrant(t, repo, org, "doc-2", "user-a")
	mustGrant(t, repo, org, "doc-3", "user-b")

	vis, err := repo.ListVisible(ctx, org, "document", "user", "user-a")
	if err != nil {
		t.Fatalf("ListVisible(a): %v", err)
	}
	if got := toSet(vis.IDs); len(got) != 2 || !got["doc-1"] || !got["doc-2"] {
		t.Fatalf("user-a visible = %v; want {doc-1,doc-2}", vis.IDs)
	}
	if vis.AllOrg {
		t.Fatal("user-a should not have all-org sentinel")
	}

	// user-b cannot see user-a's grants.
	visB, err := repo.ListVisible(ctx, org, "document", "user", "user-b")
	if err != nil {
		t.Fatalf("ListVisible(b): %v", err)
	}
	if got := toSet(visB.IDs); len(got) != 1 || !got["doc-3"] {
		t.Fatalf("user-b visible = %v; want {doc-3}", visB.IDs)
	}

	// BatchCheck only returns the ids the subject is actually granted.
	roles, err := repo.BatchCheck(ctx, org, "document", "user", "user-a", []string{"doc-1", "doc-2", "doc-3", "doc-missing"})
	if err != nil {
		t.Fatalf("BatchCheck: %v", err)
	}
	if len(roles) != 2 || roles["doc-1"] == "" || roles["doc-2"] == "" {
		t.Fatalf("BatchCheck(a) = %v; want only doc-1, doc-2", roles)
	}
	if _, leaked := roles["doc-3"]; leaked {
		t.Fatal("BatchCheck leaked another user's grant (doc-3)")
	}

	// RevokeAllForSubject clears every inbound grant for a user (GDPR path).
	n, err := repo.RevokeAllForSubject(ctx, org, "user", "user-a")
	if err != nil || n != 2 {
		t.Fatalf("RevokeAllForSubject = (%d, %v); want (2, nil)", n, err)
	}
	if vis, _ := repo.ListVisible(ctx, org, "document", "user", "user-a"); len(vis.IDs) != 0 {
		t.Fatalf("after RevokeAllForSubject user-a still sees %v", vis.IDs)
	}
}

func TestRevokeAllGrantsForUserErasure(t *testing.T) {
	repo, org, cleanup := newTestAclRepo(t)
	defer cleanup()
	ctx := context.Background()

	// user-a holds two grants; user-b holds one.
	mustGrant(t, repo, org, "doc-1", "user-a")
	mustGrant(t, repo, org, "doc-2", "user-a")
	mustGrant(t, repo, org, "doc-3", "user-b")

	// Erase user-a: every grant they hold is revoked, user-b is untouched.
	n, err := repo.RevokeAllGrantsForUser(ctx, "user-a")
	if err != nil || n != 2 {
		t.Fatalf("RevokeAllGrantsForUser(user-a) = (%d, %v); want (2, nil)", n, err)
	}
	if vis, _ := repo.ListVisible(ctx, org, "document", "user", "user-a"); len(vis.IDs) != 0 {
		t.Fatalf("erased user still has %d grants", len(vis.IDs))
	}
	if vis, _ := repo.ListVisible(ctx, org, "document", "user", "user-b"); len(vis.IDs) != 1 {
		t.Fatalf("erasing user-a affected user-b's grants: got %d, want 1", len(vis.IDs))
	}
}

func TestGrantRejectsTeamSharedType(t *testing.T) {
	repo, org, cleanup := newTestAclRepo(t)
	defer cleanup()
	ctx := context.Background()

	// A per-user grant on a team-shared type (inbox) must be rejected outright.
	if _, err := repo.Grant(ctx, &users.ResourceGrant{
		OrgID: org, ResourceType: "inbox", ResourceID: "inbox-1",
		SubjectType: "user", SubjectID: "user-a", Role: "view",
	}); err == nil {
		t.Fatal("Grant on team-shared type 'inbox' should be rejected by the taxonomy guard")
	}
	// Unknown type also rejected (fail closed).
	if _, err := repo.Grant(ctx, &users.ResourceGrant{
		OrgID: org, ResourceType: "made_up", ResourceID: "x",
		SubjectType: "user", SubjectID: "user-a", Role: "view",
	}); err == nil {
		t.Fatal("Grant on unknown type should be rejected")
	}
}

// TestCrossOrgGrantIsolation proves org_id is a hard boundary on
// resource_grants: two orgs granting the identical
// resource_type/resource_id/subject pairing must not see, list, or revoke
// each other's rows. The other tests in this file (TestGrantCheckRevoke,
// TestListVisibleAndBatchCheckScopePerUser, ...) only ever exercise a single
// org, so per-user scoping was proven but cross-org scoping was not.
func TestCrossOrgGrantIsolation(t *testing.T) {
	repo, orgA, cleanup := newTestAclRepo(t)
	defer cleanup()
	ctx := context.Background()

	orgB := "owntest-" + uuid.NewString()
	defer func() {
		_ = repo.Revoke(context.Background(), orgB, "document", "doc-1", "user", "user-a")
	}()

	// Same resource_type/resource_id/subject pairing granted independently in
	// each org. The unique index is (org_id, resource_type, resource_id,
	// subject_type, subject_id), so if org_id scoping were dropped anywhere
	// along the read path these two rows would be indistinguishable.
	mustGrant(t, repo, orgA, "doc-1", "user-a")
	mustGrant(t, repo, orgB, "doc-1", "user-a")

	// Check must key off org_id, not just resource+subject.
	if okA, roleA, err := repo.Check(ctx, orgA, "document", "doc-1", "user", "user-a"); err != nil || !okA || roleA != "view" {
		t.Fatalf("Check(orgA) = (%v,%q,err=%v); want (true,view,nil)", okA, roleA, err)
	}

	// ListVisible for org A must not include org B's identically-keyed grant.
	visA, err := repo.ListVisible(ctx, orgA, "document", "user", "user-a")
	if err != nil {
		t.Fatalf("ListVisible(orgA): %v", err)
	}
	if got := toSet(visA.IDs); len(got) != 1 || !got["doc-1"] {
		t.Fatalf("ListVisible(orgA) = %v; want exactly {doc-1} (must not include org B's rows)", visA.IDs)
	}

	// ListByResource for org A must not surface org B's grant on the same
	// resource_id.
	grantsA, err := repo.ListByResource(ctx, orgA, "document", "doc-1")
	if err != nil {
		t.Fatalf("ListByResource(orgA): %v", err)
	}
	if len(grantsA) != 1 || grantsA[0].OrgID != orgA {
		t.Fatalf("ListByResource(orgA) leaked cross-org grants: %+v", grantsA)
	}

	// Revoking org A's grant must not touch org B's identically-keyed grant.
	if err := repo.Revoke(ctx, orgA, "document", "doc-1", "user", "user-a"); err != nil {
		t.Fatalf("Revoke(orgA): %v", err)
	}
	if okB, roleB, err := repo.Check(ctx, orgB, "document", "doc-1", "user", "user-a"); err != nil || !okB || roleB != "view" {
		t.Fatalf("Check(orgB) after orgA revoke = (%v,%q,err=%v); want (true,view,nil) — cross-org revoke leak", okB, roleB, err)
	}
}

func mustGrant(t *testing.T, repo *users.AclRepository, org, doc, user string) {
	t.Helper()
	if _, err := repo.Grant(context.Background(), &users.ResourceGrant{
		OrgID: org, ResourceType: "document", ResourceID: doc,
		SubjectType: "user", SubjectID: user, Role: "view",
	}); err != nil {
		t.Fatalf("mustGrant(%s,%s): %v", doc, user, err)
	}
}

func toSet(ids []string) map[string]bool {
	s := make(map[string]bool, len(ids))
	for _, id := range ids {
		s[id] = true
	}
	return s
}

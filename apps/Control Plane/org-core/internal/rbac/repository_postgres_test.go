package rbac

import (
	"context"
	"errors"
	"os"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

// TestControlLifecycleRoleRepositoryIsOrgScopedUnderRLS proves the fix for
// the audited gap: internal/rbac/repository.go now routes every query
// through database.DB.WithOrgScope (SET LOCAL app.current_org + SET LOCAL
// ROLE org_core_app), the same mechanism internal/org/repository.go's
// single-tenant paths already rely on, instead of querying the raw
// superuser pool directly. This test exercises the full CRUD + member-role
// surface against a real, migrated Postgres and specifically asserts that
// operations scoped to one org cannot see or mutate another org's roles —
// the property that would silently break if a future edit swapped
// WithOrgScope back out for a bare pool call.
func TestControlLifecycleRoleRepositoryIsOrgScopedUnderRLS(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(
		ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID"),
	); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	const orgA = "org-rbac-lifecycle-a"
	const orgB = "org-rbac-lifecycle-b"
	for _, id := range []string{orgA, orgB} {
		// Migration 010's deferred constraint trigger requires every live
		// organization to have an active owner by COMMIT — seed both rows
		// in one transaction so the check passes.
		tx, err := db.Pool.Begin(ctx)
		if err != nil {
			t.Fatalf("begin seed tx for %s: %v", id, err)
		}
		if _, err := tx.Exec(ctx,
			`INSERT INTO organizations (id, name) VALUES ($1, $2)
			 ON CONFLICT (id) DO NOTHING`, id, "RBAC Lifecycle "+id,
		); err != nil {
			t.Fatalf("seed organization %s: %v", id, err)
		}
		if _, err := tx.Exec(ctx,
			`INSERT INTO organization_members (org_id, user_id, role, status) VALUES ($1, $2, 'owner', 'active')
			 ON CONFLICT (org_id, user_id) DO NOTHING`, id, "owner-"+id,
		); err != nil {
			t.Fatalf("seed owner for %s: %v", id, err)
		}
		if err := tx.Commit(ctx); err != nil {
			t.Fatalf("commit seed tx for %s: %v", id, err)
		}
	}
	// Non-custom seed role, inserted directly (Repository.Create only ever
	// makes is_custom=TRUE rows) so Delete's default-role guard has
	// something to reject.
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO org_role_mappings (id, org_id, role_name, permissions, is_custom)
VALUES ($1, $2, 'member', '["org:read"]'::jsonb, FALSE)
ON CONFLICT (org_id, role_name) DO NOTHING`, orgA+"_member", orgA); err != nil {
		t.Fatalf("seed default role: %v", err)
	}
	const memberUser = "user-rbac-lifecycle"
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO organization_members (org_id, user_id, role) VALUES ($1, $2, 'viewer')
ON CONFLICT (org_id, user_id) DO NOTHING`, orgA, memberUser); err != nil {
		t.Fatalf("seed organization member: %v", err)
	}

	repo := NewRepository(db)

	created, err := repo.Create(ctx, CreateParams{OrgID: orgA, RoleName: "custom-a", Permissions: []string{"org:read"}})
	if err != nil {
		t.Fatalf("create custom role: %v", err)
	}
	if created.OrgID != orgA || created.RoleName != "custom-a" || !created.IsCustom {
		t.Fatalf("unexpected created role: %+v", created)
	}

	if _, err := repo.Create(ctx, CreateParams{OrgID: orgA, RoleName: "custom-a", Permissions: []string{"org:read"}}); !errors.Is(err, ErrAlreadyExists) {
		t.Fatalf("duplicate create err=%v; want ErrAlreadyExists", err)
	}

	if _, err := repo.Create(ctx, CreateParams{OrgID: orgA, RoleName: "bad-caps", Permissions: []string{"not:a:real:capability"}}); !errors.Is(err, ErrInvalidCapability) {
		t.Fatalf("invalid capability create err=%v; want ErrInvalidCapability", err)
	}

	// Cross-org isolation: a role created for org A must not be visible
	// when queried under org B's scope, even though the row exists in the
	// same shared table.
	if _, err := repo.Get(ctx, orgB, "custom-a"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("cross-org get err=%v; want ErrNotFound", err)
	}
	got, err := repo.Get(ctx, orgA, "custom-a")
	if err != nil || got.RoleName != "custom-a" {
		t.Fatalf("same-org get role=%+v err=%v", got, err)
	}

	listA, err := repo.List(ctx, orgA)
	if err != nil {
		t.Fatalf("list org A: %v", err)
	}
	if len(listA) != 2 { // seeded "member" + created "custom-a"
		t.Fatalf("list org A = %+v; want 2 roles", listA)
	}
	listB, err := repo.List(ctx, orgB)
	if err != nil {
		t.Fatalf("list org B: %v", err)
	}
	if len(listB) != 0 {
		t.Fatalf("list org B = %+v; want 0 roles (no cross-org leakage)", listB)
	}

	// Updating the same role_name under the wrong org must not touch org A's row.
	if _, err := repo.Update(ctx, UpdateParams{OrgID: orgB, RoleName: "custom-a", Permissions: []string{"org:read"}}); !errors.Is(err, ErrNotFound) {
		t.Fatalf("cross-org update err=%v; want ErrNotFound", err)
	}
	updated, err := repo.Update(ctx, UpdateParams{OrgID: orgA, RoleName: "custom-a", Permissions: []string{"org:read", "resources:read"}})
	if err != nil || len(updated.Permissions) != 2 {
		t.Fatalf("same-org update role=%+v err=%v", updated, err)
	}

	// Deleting a non-custom (seeded) role must fail closed; deleting a
	// custom role under the wrong org must report not-found, not silently
	// delete org A's row.
	if err := repo.Delete(ctx, orgA, "member"); !errors.Is(err, ErrCannotDelete) {
		t.Fatalf("delete default role err=%v; want ErrCannotDelete", err)
	}
	if err := repo.Delete(ctx, orgB, "custom-a"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("cross-org delete err=%v; want ErrNotFound", err)
	}
	if err := repo.Delete(ctx, orgA, "custom-a"); err != nil {
		t.Fatalf("same-org delete: %v", err)
	}
	if _, err := repo.Get(ctx, orgA, "custom-a"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("get after delete err=%v; want ErrNotFound", err)
	}

	// AssignMemberRole: role-existence check and the membership update run
	// inside the same org-scoped transaction. A role defined only for org A
	// must not resolve when the caller claims org B, even for a user id
	// that (if unscoped) happens to also exist under org A.
	assignment, err := repo.AssignMemberRole(ctx, orgA, memberUser, "member")
	if err != nil || assignment.Role != "member" {
		t.Fatalf("same-org assign role=%+v err=%v", assignment, err)
	}
	if _, err := repo.AssignMemberRole(ctx, orgB, memberUser, "member"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("cross-org assign err=%v; want ErrNotFound (role not defined for org B)", err)
	}
	if _, err := repo.AssignMemberRole(ctx, orgA, "user-not-a-member", "member"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("assign to non-member err=%v; want ErrNotFound", err)
	}
}

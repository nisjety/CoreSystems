package spaces

import (
	"context"
	"os"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// A roster sync must never demote the Space owner.
//
// This is a regression test for a defect the live system produced: the
// organization roster naturally CONTAINS the owner — they are an ordinary
// member of the organization too — and the sync maps every member to `editor`.
// So the first sync silently downgraded the owner in their own Space, and no
// later sync could restore them, because the same source list kept naming the
// lower role. Preserving the owner's presence was not enough; the realistic
// harm was demotion, not removal.
func TestReplaceMembershipsNeverDemotesTheOwner(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live membership test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer pool.Close()
	repo := &Repository{db: &database.DB{Pool: pool}}

	const (
		spaceRef = "test-space-owner-guard"
		orgID    = "test-org-owner-guard"
		owner    = "test-owner-principal"
		other    = "test-member-principal"
	)
	t.Cleanup(func() {
		_, _ = pool.Exec(ctx, `DELETE FROM space_memberships WHERE space_ref=$1`, spaceRef)
		_, _ = pool.Exec(ctx, `DELETE FROM space_authority_revisions WHERE space_ref=$1`, spaceRef)
		_, _ = pool.Exec(ctx, `DELETE FROM registered_spaces WHERE space_ref=$1`, spaceRef)
	})

	if _, err := pool.Exec(ctx, `
		INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
		    application_lifecycle_revision, registration_state)
		VALUES ($1,$2,'room',$3,1,'active')
		ON CONFLICT (space_ref) DO NOTHING`, spaceRef, orgID, owner); err != nil {
		t.Fatalf("seed Space: %v", err)
	}
	if _, err := pool.Exec(ctx,
		`INSERT INTO space_authority_revisions (space_ref) VALUES ($1) ON CONFLICT DO NOTHING`, spaceRef); err != nil {
		t.Fatalf("seed revisions: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by)
		VALUES ($1,'user',$2,'owner','test')
		ON CONFLICT (space_ref, subject_type, subject_id) DO UPDATE SET role='owner', active=TRUE`,
		spaceRef, owner); err != nil {
		t.Fatalf("seed owner membership: %v", err)
	}

	// The roster names the owner as a plain member, exactly as org-core's list
	// does, plus one genuine member.
	if _, err := repo.ReplaceMemberships(ctx, MembershipReplacement{
		SpaceRef: spaceRef,
		Members: []MemberGrant{
			{SubjectType: "user", SubjectID: owner, Role: "editor"},
			{SubjectType: "user", SubjectID: other, Role: "editor"},
		},
	}); err != nil {
		t.Fatalf("replace memberships: %v", err)
	}

	var role string
	var active bool
	if err := pool.QueryRow(ctx,
		`SELECT role, active FROM space_memberships WHERE space_ref=$1 AND subject_id=$2`,
		spaceRef, owner).Scan(&role, &active); err != nil {
		t.Fatalf("read owner membership: %v", err)
	}
	if role != "owner" || !active {
		t.Fatalf("owner was changed by a roster sync: role=%q active=%v", role, active)
	}

	if err := pool.QueryRow(ctx,
		`SELECT role, active FROM space_memberships WHERE space_ref=$1 AND subject_id=$2`,
		spaceRef, other).Scan(&role, &active); err != nil {
		t.Fatalf("read member: %v", err)
	}
	if role != "editor" || !active {
		t.Fatalf("declared member was not applied: role=%q active=%v", role, active)
	}

	// Absence revokes an ordinary member but still leaves the owner alone.
	if _, err := repo.ReplaceMemberships(ctx, MembershipReplacement{SpaceRef: spaceRef}); err != nil {
		t.Fatalf("empty replace: %v", err)
	}
	if err := pool.QueryRow(ctx,
		`SELECT role, active FROM space_memberships WHERE space_ref=$1 AND subject_id=$2`,
		spaceRef, other).Scan(&role, &active); err != nil {
		t.Fatalf("read revoked member: %v", err)
	}
	if active {
		t.Fatal("an absent member stayed active")
	}
	if err := pool.QueryRow(ctx,
		`SELECT role, active FROM space_memberships WHERE space_ref=$1 AND subject_id=$2`,
		spaceRef, owner).Scan(&role, &active); err != nil {
		t.Fatalf("read owner after empty replace: %v", err)
	}
	if role != "owner" || !active {
		t.Fatalf("an empty roster removed the owner: role=%q active=%v", role, active)
	}
}

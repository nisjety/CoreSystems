package spaces

import (
	"context"
	"os"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
)

// seedMembershipTestSpace registers a test Space plus its authority-revision
// row, and schedules cleanup of every row the test created.
func seedMembershipTestSpace(t *testing.T, pool *pgxpool.Pool, spaceRef, orgID, owner string) {
	t.Helper()
	ctx := context.Background()
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
}

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
	seedMembershipTestSpace(t, pool, spaceRef, orgID, owner)

	if _, err := pool.Exec(ctx, `
		INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by)
		VALUES ($1,'user',$2,'owner','test')
		ON CONFLICT (space_ref, subject_type, subject_id) DO UPDATE SET role='owner', active=TRUE`,
		spaceRef, owner); err != nil {
		t.Fatalf("seed owner membership: %v", err)
	}

	// The roster names the owner as a plain member, exactly as org-core's list
	// does, plus one genuine member.
	if _, revoked, reactivated, gotOrgID, err := repo.ReplaceMemberships(ctx, MembershipReplacement{
		SpaceRef: spaceRef,
		Members: []MemberGrant{
			{SubjectType: "user", SubjectID: owner, Role: "editor"},
			{SubjectType: "user", SubjectID: other, Role: "editor"},
		},
	}); err != nil {
		t.Fatalf("replace memberships: %v", err)
	} else if len(revoked) != 0 {
		t.Fatalf("a converging (non-revoking) sync reported revoked subjects: %v", revoked)
	} else if len(reactivated) != 0 {
		t.Fatalf("a freshly-added member was reported as reactivated: %v", reactivated)
	} else if gotOrgID != orgID {
		t.Fatalf("org_id = %q, want %q", gotOrgID, orgID)
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
	// The revocation is reported so the caller can fan it out cross-plane.
	// The owner must never appear here even though they were also absent
	// from this empty roster.
	if _, revoked, reactivated, _, err := repo.ReplaceMemberships(ctx, MembershipReplacement{SpaceRef: spaceRef}); err != nil {
		t.Fatalf("empty replace: %v", err)
	} else if len(revoked) != 1 || revoked[0] != other {
		t.Fatalf("revoked subjects = %v, want exactly [%q]", revoked, other)
	} else if len(reactivated) != 0 {
		t.Fatalf("an empty replace unexpectedly reported a reactivation: %v", reactivated)
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

	// The revoked member rejoins: a legitimate rejoin must be reported as a
	// reactivation (so the caller can clear the cross-plane revocation it
	// published above), never as another revocation.
	if _, revoked, reactivated, _, err := repo.ReplaceMemberships(ctx, MembershipReplacement{
		SpaceRef: spaceRef,
		Members: []MemberGrant{
			{SubjectType: "user", SubjectID: owner, Role: "owner"},
			{SubjectType: "user", SubjectID: other, Role: "editor"},
		},
	}); err != nil {
		t.Fatalf("rejoin replace: %v", err)
	} else if len(revoked) != 0 {
		t.Fatalf("a rejoin was unexpectedly also reported as a revocation: %v", revoked)
	} else if len(reactivated) != 1 || reactivated[0] != other {
		t.Fatalf("reactivated subjects = %v, want exactly [%q]", reactivated, other)
	}
	if err := pool.QueryRow(ctx,
		`SELECT role, active FROM space_memberships WHERE space_ref=$1 AND subject_id=$2`,
		spaceRef, other).Scan(&role, &active); err != nil {
		t.Fatalf("read rejoined member: %v", err)
	}
	if role != "editor" || !active {
		t.Fatalf("rejoined member was not restored: role=%q active=%v", role, active)
	}
}

// A human-roster sync must not revoke the room's agents. org-core knows people
// and nothing else, so an unscoped convergence from it deactivates every bound
// agent as a side effect — a Space agent binding destroyed by a sync that never
// knew the binding existed.
func TestUserScopedReplacementLeavesAgentsBound(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live membership scope test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer pool.Close()
	repo := &Repository{db: &database.DB{Pool: pool}}

	const (
		spaceRef = "test-space-agent-scope"
		orgID    = "test-org-agent-scope"
		owner    = "test-owner-agent-scope"
		agent    = "test-agent-subject"
	)
	seedMembershipTestSpace(t, pool, spaceRef, orgID, owner)

	// An agent bound to the room by a different, agent-aware decision.
	if _, err := pool.Exec(ctx, `
		INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by)
		VALUES ($1,'service',$2,'editor','test')
		ON CONFLICT (space_ref, subject_type, subject_id) DO UPDATE SET active=TRUE`,
		spaceRef, agent); err != nil {
		t.Fatalf("seed agent membership: %v", err)
	}

	// The human roster converges and never mentions the agent.
	if _, _, _, _, err := repo.ReplaceMemberships(ctx, MembershipReplacement{
		SpaceRef:            spaceRef,
		ManagedSubjectTypes: []string{"user"},
		Members: []MemberGrant{
			{SubjectType: "user", SubjectID: owner, Role: "owner"},
		},
	}); err != nil {
		t.Fatalf("replace memberships: %v", err)
	}

	var active bool
	if err := pool.QueryRow(ctx,
		`SELECT active FROM space_memberships WHERE space_ref=$1 AND subject_type='service' AND subject_id=$2`,
		spaceRef, agent).Scan(&active); err != nil {
		t.Fatalf("read agent membership: %v", err)
	}
	if !active {
		t.Fatal("a human roster sync revoked the room's agent")
	}

	// Without a declared scope the same call still owns the whole roster, so an
	// agent-aware caller can genuinely revoke one. The revoked-subjects report
	// is user-scoped: revoking a service-type agent must never surface here,
	// since the cross-plane consumer this feeds only invalidates human
	// resource-scoped authorization.
	if _, revoked, _, _, err := repo.ReplaceMemberships(ctx, MembershipReplacement{
		SpaceRef: spaceRef,
		Members: []MemberGrant{
			{SubjectType: "user", SubjectID: owner, Role: "owner"},
		},
	}); err != nil {
		t.Fatalf("unscoped replace: %v", err)
	} else if len(revoked) != 0 {
		t.Fatalf("a service-subject revocation was reported as a user revocation: %v", revoked)
	}
	if err := pool.QueryRow(ctx,
		`SELECT active FROM space_memberships WHERE space_ref=$1 AND subject_type='service' AND subject_id=$2`,
		spaceRef, agent).Scan(&active); err != nil {
		t.Fatalf("read agent membership after unscoped replace: %v", err)
	}
	if active {
		t.Fatal("an unscoped replacement must still be able to revoke an agent")
	}
}

// The index answers with the Spaces a subject actually belongs to, and the
// organization-membership backstop removes them all when the person leaves the
// organization — even before the per-Space revocation syncs.
func TestSpacesForSubjectIsActorFiltered(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live index test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer pool.Close()
	repo := &Repository{db: &database.DB{Pool: pool}}

	// A subject with no organization membership sees nothing, regardless of
	// what Spaces exist.
	entries, err := repo.SpacesForSubject(ctx, "test-org-index", "test-stranger")
	if err != nil {
		t.Fatalf("index for stranger: %v", err)
	}
	if len(entries) != 0 {
		t.Fatalf("a non-member saw %d Spaces", len(entries))
	}

	// Missing identity is refused rather than treated as "everyone".
	if _, err := repo.SpacesForSubject(ctx, "", "test-stranger"); err == nil {
		t.Fatal("an empty organization was accepted")
	}
	if _, err := repo.SpacesForSubject(ctx, "test-org-index", ""); err == nil {
		t.Fatal("an empty subject was accepted")
	}
}

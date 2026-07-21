package org

import (
	"context"
	"fmt"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

// TestControlLifecycleSuccessionPromotesOnlyAnExistingActiveMember is the
// admin-succession handoff (design doc Flow B) exercised against a disposable
// Postgres: a departing sole owner nominates an existing active member, and
// Repository.PromoteMember / Service.PromoteMemberSuccession must promote
// ONLY that member — never invite, never remove, never touch anyone else —
// while the DB-level owner-invariant constraint trigger
// (migrations/010_owner_invariant.up.sql) keeps the organization owned
// throughout.
func TestControlLifecycleSuccessionPromotesOnlyAnExistingActiveMember(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
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

	repo := NewRepository(db)
	service := NewService(repo, nil)
	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	orgID := "org-succession-" + suffix
	ownerID := "owner-succession-" + suffix
	memberID := "member-succession-" + suffix
	strangerID := "stranger-succession-" + suffix

	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
		ID: orgID, Name: "Succession Org", Plan: "free", Status: "active",
	}, ownerID); err != nil {
		t.Fatalf("provision organization: %v", err)
	}
	if err := repo.AddOrganizationMember(ctx, orgID, memberID, "member"); err != nil {
		t.Fatalf("seed member: %v", err)
	}

	// Invalid role is rejected before any database access.
	if err := service.PromoteMemberSuccession(ctx, orgID, memberID, "viewer"); err == nil {
		t.Fatal("PromoteMemberSuccession accepted a non-base role")
	}
	if err := service.PromoteMemberSuccession(ctx, "", memberID, "owner"); err == nil {
		t.Fatal("PromoteMemberSuccession accepted a blank organization id")
	}
	if err := service.PromoteMemberSuccession(ctx, orgID, "", "owner"); err == nil {
		t.Fatal("PromoteMemberSuccession accepted a blank user id")
	}

	// Promoting a user who is not an active member of the org fails closed.
	if err := service.PromoteMemberSuccession(ctx, orgID, strangerID, "owner"); err == nil {
		t.Fatal("PromoteMemberSuccession promoted a non-member")
	}

	// The real handoff: the existing member becomes owner.
	if err := service.PromoteMemberSuccession(ctx, orgID, memberID, "owner"); err != nil {
		t.Fatalf("PromoteMemberSuccession(member -> owner) failed: %v", err)
	}
	members, err := repo.ListOrganizationMembers(ctx, orgID)
	if err != nil {
		t.Fatalf("list organization members: %v", err)
	}
	roles := map[string]string{}
	for _, m := range members {
		roles[m.UserID] = m.Role
	}
	if roles[memberID] != "owner" {
		t.Fatalf("member role = %q, want owner", roles[memberID])
	}
	if roles[ownerID] != "owner" {
		t.Fatalf("original owner role changed to %q; PromoteMember must never touch other members", roles[ownerID])
	}
	if len(members) != 2 {
		t.Fatalf("member count = %d, want 2 (PromoteMember must never invite/remove)", len(members))
	}

	// Removing the original owner is still safe: the org now has two owners.
	if err := repo.RemoveOrganizationMember(ctx, orgID, ownerID); err != nil {
		t.Fatalf("remove original owner after successful handoff: %v", err)
	}

	// A removed (non-active) member can no longer be promoted.
	if err := repo.PromoteMember(ctx, orgID, ownerID, "admin"); err == nil {
		t.Fatal("PromoteMember promoted a removed (inactive) member")
	} else if !strings.Contains(err.Error(), "no active membership") {
		t.Fatalf("unexpected error promoting removed member: %v", err)
	}
}

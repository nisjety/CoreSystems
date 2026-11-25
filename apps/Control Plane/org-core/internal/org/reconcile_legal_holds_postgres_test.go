package org

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

// TestControlLifecycleReconcileKeepsAHeldOrganization proves F-f10-06 at the Auth Core
// reconcile entry point against a disposable Postgres (the org-core migrations and the
// reconcile gate). With ERASURE_HOLD_CHECK on, a reconcile of an organisation under an active org
// hold is refused with ErasureHeldError: the organisation and its hold survive, and the
// tombstone stays incomplete. Once the hold is released, the same revision erases it. With
// the switch off, the same reconcile erases the organisation as it always did.
func TestControlLifecycleReconcileKeepsAHeldOrganization(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
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
	holds := retention.NewRepository(db)
	compliance := retention.Actor{ID: "u-comp", Role: "compliance"}
	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	const revision = 10

	seed := func(label string) string {
		t.Helper()
		orgID := "org-reconcile-hold-" + label + "-" + suffix
		if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
			ID: orgID, Name: "Reconcile Hold " + label, Plan: "free", Status: "active",
		}, "owner-reconcile-hold-"+label+"-"+suffix); err != nil {
			t.Fatalf("provision %s: %v", label, err)
		}
		return orgID
	}
	placeOrgHold := func(orgID string) retention.Hold {
		t.Helper()
		var raw [16]byte
		if _, err := rand.Read(raw[:]); err != nil {
			t.Fatalf("hold id: %v", err)
		}
		result, err := holds.PlaceHold(ctx, orgID, compliance, retention.Hold{
			ID:             "lh_" + hex.EncodeToString(raw[:]),
			OrganizationID: orgID,
			Scope:          retention.ScopeOrg,
			TargetRef:      orgID,
			HoldRef:        "Matter reconcile-hold",
			CreatedBy:      compliance.ID,
		})
		if err != nil {
			t.Fatalf("place org hold on %s: %v", orgID, err)
		}
		return result.Hold
	}
	countRows := func(query string, args ...any) int {
		t.Helper()
		var n int
		if err := db.Pool.QueryRow(ctx, query, args...).Scan(&n); err != nil {
			t.Fatalf("count rows: %v", err)
		}
		return n
	}

	// Switch on: a held organisation is refused, and nothing is erased.
	t.Setenv(envErasureHoldCheck, "true")
	heldOrg := seed("held")
	hold := placeOrgHold(heldOrg)
	receipt, applied, err := repo.ReconcileOrganizationDeletion(ctx, heldOrg, revision)
	var held *ErasureHeldError
	if !errors.As(err, &held) || held.OrgHolds != 1 || held.ThreadHolds != 0 || !errors.Is(err, ErrErasureBlockedLegalHold) {
		t.Fatalf("held reconcile: receipt=%s applied=%t err=%v; want ErasureHeldError{1,0}", receipt, applied, err)
	}
	if applied || receipt != nil {
		t.Fatalf("refused reconcile reported receipt=%s applied=%t", receipt, applied)
	}
	if n := countRows(`SELECT COUNT(*)::INT FROM organizations WHERE id = $1`, heldOrg); n != 1 {
		t.Fatalf("organisation rows after refused erasure = %d, want 1", n)
	}
	if n := countRows(`SELECT COUNT(*)::INT FROM org_legal_holds WHERE id = $1 AND released_at IS NULL`, hold.ID); n != 1 {
		t.Fatalf("active holds after refused erasure = %d, want 1", n)
	}
	if n := countRows(`SELECT COUNT(*)::INT FROM auth_organization_tombstones WHERE org_id = $1 AND revision = $2 AND erasure_completed_at IS NULL`, heldOrg, revision); n != 1 {
		t.Fatalf("incomplete tombstone at revision %d = %d rows, want 1", revision, n)
	}

	// Released: the same revision, retried by the outbox, resumes and erases the organisation.
	if _, err := holds.ReleaseHold(ctx, heldOrg, hold.ID, compliance); err != nil {
		t.Fatalf("release org hold: %v", err)
	}
	receipt, applied, err = repo.ReconcileOrganizationDeletion(ctx, heldOrg, revision)
	if err != nil || !applied {
		t.Fatalf("released-hold retry: applied=%t receipt=%s err=%v; want the erasure applied", applied, receipt, err)
	}
	if n := countRows(`SELECT COUNT(*)::INT FROM organizations WHERE id = $1`, heldOrg); n != 0 {
		t.Fatalf("organisation rows after released erasure = %d, want 0", n)
	}

	// Switch off: the same reconcile erases an organisation even under an active hold, as before.
	t.Setenv(envErasureHoldCheck, "")
	plainOrg := seed("switch-off")
	placeOrgHold(plainOrg)
	receipt, applied, err = repo.ReconcileOrganizationDeletion(ctx, plainOrg, revision)
	if err != nil || !applied {
		t.Fatalf("switch-off reconcile: applied=%t receipt=%s err=%v; want the erasure as before", applied, receipt, err)
	}
	if n := countRows(`SELECT COUNT(*)::INT FROM organizations WHERE id = $1`, plainOrg); n != 0 {
		t.Fatalf("organisation rows with the switch off = %d, want 0", n)
	}
}

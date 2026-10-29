package org

import (
	"context"
	"errors"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

// TestControlLifecycleDeletionLedgerIsIdempotentAndOrgScoped exercises the
// Flow C org-deletion ledger (migrations/018_org_deletion_ledger.up.sql)
// against a disposable Postgres: ledger creation survives a duplicated
// soft-delete trigger (at-least-once redelivery), Mark*/List stay scoped to
// their own org, restore clears both deleted_at and the ledger, restoring an
// already-active org fails closed, and the reminder-selection queries return
// only organizations that are actually due and not yet reminded.
func TestControlLifecycleDeletionLedgerIsIdempotentAndOrgScoped(t *testing.T) {
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
	suffix := fmt.Sprintf("%d", time.Now().UnixNano())

	seedOrg := func(label string) (orgID, ownerID, memberID string) {
		t.Helper()
		orgID = "org-deletion-ledger-" + label + "-" + suffix
		ownerID = "owner-deletion-ledger-" + label + "-" + suffix
		memberID = "member-deletion-ledger-" + label + "-" + suffix
		if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
			ID: orgID, Name: "Deletion Ledger " + label, Plan: "free", Status: "active",
		}, ownerID); err != nil {
			t.Fatalf("provision %s organization: %v", label, err)
		}
		if err := repo.AddOrganizationMember(ctx, orgID, memberID, "member"); err != nil {
			t.Fatalf("seed %s member: %v", label, err)
		}
		return orgID, ownerID, memberID
	}

	orgA, ownerA, memberA := seedOrg("a")
	orgB, ownerB, _ := seedOrg("b")

	// --- Ledger creation is idempotent on double-trigger -------------------
	membersA := []string{ownerA, memberA}
	if err := repo.CreateDeletionLedger(ctx, orgA, membersA); err != nil {
		t.Fatalf("create deletion ledger (first trigger): %v", err)
	}
	if err := repo.CreateDeletionLedger(ctx, orgA, membersA); err != nil {
		t.Fatalf("create deletion ledger (re-trigger / at-least-once redelivery): %v", err)
	}
	entries, err := repo.ListDeletionLedger(ctx, orgA)
	if err != nil {
		t.Fatalf("list deletion ledger: %v", err)
	}
	if len(entries) != 2 {
		t.Fatalf("deletion ledger rows = %d, want 2 (double-trigger must not duplicate rows)", len(entries))
	}
	for _, e := range entries {
		if e.NotifiedAt == nil {
			t.Fatalf("member %s: notified_at not stamped at ledger creation", e.UserID)
		}
		if e.ExportedAt != nil || e.AcknowledgedAt != nil {
			t.Fatalf("member %s: exported_at/acknowledged_at set before any Mark* call", e.UserID)
		}
	}

	// --- MarkExported / MarkAcknowledged stay scoped to their own org ------
	if err := repo.MarkExported(ctx, orgA, ownerA); err != nil {
		t.Fatalf("mark exported: %v", err)
	}
	if err := repo.MarkAcknowledged(ctx, orgA, memberA); err != nil {
		t.Fatalf("mark acknowledged: %v", err)
	}
	// ownerB is a real user, but not a member of orgA's ledger — must not be
	// reachable through orgA's scope (no cross-org bleed).
	if err := repo.MarkExported(ctx, orgA, ownerB); !errors.Is(err, ErrNotFound) {
		t.Fatalf("mark exported for a foreign org's member: err=%v, want ErrNotFound", err)
	}
	entries, err = repo.ListDeletionLedger(ctx, orgA)
	if err != nil {
		t.Fatalf("list deletion ledger after marks: %v", err)
	}
	byUser := map[string]DeletionLedgerEntry{}
	for _, e := range entries {
		byUser[e.UserID] = e
	}
	if byUser[ownerA].ExportedAt == nil {
		t.Fatal("owner's exported_at was not stamped")
	}
	if byUser[memberA].AcknowledgedAt == nil {
		t.Fatal("member's acknowledged_at was not stamped")
	}
	if byUser[memberA].ExportedAt != nil {
		t.Fatal("member's exported_at was stamped by a call scoped to the owner")
	}

	// orgB never had a ledger created for it — isolation, not just an empty result.
	entriesB, err := repo.ListDeletionLedger(ctx, orgB)
	if err != nil {
		t.Fatalf("list orgB deletion ledger: %v", err)
	}
	if len(entriesB) != 0 {
		t.Fatalf("orgB deletion ledger rows = %d, want 0 (isolation breach)", len(entriesB))
	}

	// --- GetDeletionStatus before/after soft-delete -------------------------
	status, err := repo.GetDeletionStatus(ctx, orgA)
	if err != nil {
		t.Fatalf("get deletion status (active org): %v", err)
	}
	if status.DeletedAt != nil {
		t.Fatal("active organization reported as pending deletion")
	}
	if _, err := repo.GetDeletionStatus(ctx, "org-does-not-exist-"+suffix); !errors.Is(err, ErrNotFound) {
		t.Fatalf("get deletion status for missing org: err=%v, want ErrNotFound", err)
	}

	// --- Restore on an already-active org is a no-op that fails closed -----
	if err := repo.RestoreOrganization(ctx, orgA); !errors.Is(err, ErrOrganizationNotPendingDeletion) {
		t.Fatalf("restore active organization: err=%v, want ErrOrganizationNotPendingDeletion", err)
	}

	// Soft-delete orgA directly (bypassing the HTTP/service layer, which is a
	// separate build stage) so restore has something real to reverse.
	if _, err := db.Pool.Exec(ctx,
		`UPDATE organizations SET deleted_at = NOW(), status = 'deleted' WHERE id = $1`, orgA,
	); err != nil {
		t.Fatalf("fixture soft-delete orgA: %v", err)
	}

	status, err = repo.GetDeletionStatus(ctx, orgA)
	if err != nil {
		t.Fatalf("get deletion status (pending org): %v", err)
	}
	if status.DeletedAt == nil {
		t.Fatal("soft-deleted organization reported as active")
	}
	if status.Name != "Deletion Ledger a" {
		t.Fatalf("deletion status name = %q, want %q", status.Name, "Deletion Ledger a")
	}

	// --- Restore clears deleted_at AND the ledger ---------------------------
	if err := repo.RestoreOrganization(ctx, orgA); err != nil {
		t.Fatalf("restore pending organization: %v", err)
	}
	if err := repo.DeleteDeletionLedger(ctx, orgA); err != nil {
		t.Fatalf("delete deletion ledger after restore: %v", err)
	}
	status, err = repo.GetDeletionStatus(ctx, orgA)
	if err != nil {
		t.Fatalf("get deletion status after restore: %v", err)
	}
	if status.DeletedAt != nil {
		t.Fatal("restored organization still reports deleted_at")
	}
	entries, err = repo.ListDeletionLedger(ctx, orgA)
	if err != nil {
		t.Fatalf("list deletion ledger after restore: %v", err)
	}
	if len(entries) != 0 {
		t.Fatalf("deletion ledger rows after restore = %d, want 0", len(entries))
	}

	// DeleteDeletionLedger on an org with no ledger rows left is a no-op, not
	// an error (idempotent restore retry under at-least-once delivery).
	if err := repo.DeleteDeletionLedger(ctx, orgA); err != nil {
		t.Fatalf("delete deletion ledger (already empty): %v", err)
	}

	// --- Reminder selection returns only orgs actually due and unreminded --
	remindDueOrgID, _, _ := seedOrg("remind-due")
	notYetDueOrgID, _, _ := seedOrg("remind-not-yet")
	alreadySentOrgID, _, _ := seedOrg("remind-already-sent")

	// 24 days elapsed: past the 23-day (30-7) threshold for the 7-day
	// reminder, short of the 29-day (30-1) threshold for the 1-day reminder.
	if _, err := db.Pool.Exec(ctx,
		`UPDATE organizations SET deleted_at = NOW() - INTERVAL '24 days', status = 'deleted' WHERE id = $1`,
		remindDueOrgID,
	); err != nil {
		t.Fatalf("fixture soft-delete (due for 7d reminder): %v", err)
	}
	// Only 10 days elapsed: not due for either reminder yet.
	if _, err := db.Pool.Exec(ctx,
		`UPDATE organizations SET deleted_at = NOW() - INTERVAL '10 days', status = 'deleted' WHERE id = $1`,
		notYetDueOrgID,
	); err != nil {
		t.Fatalf("fixture soft-delete (not yet due): %v", err)
	}
	// 25 days elapsed (also past the 7-day threshold) but its 7-day reminder
	// was already sent — must not be selected again.
	if _, err := db.Pool.Exec(ctx,
		`UPDATE organizations
		 SET deleted_at = NOW() - INTERVAL '25 days', status = 'deleted', deletion_reminder_7d_sent_at = NOW()
		 WHERE id = $1`,
		alreadySentOrgID,
	); err != nil {
		t.Fatalf("fixture soft-delete (already reminded): %v", err)
	}

	due7d, err := repo.ListOrgsNeeding7DayReminder(ctx)
	if err != nil {
		t.Fatalf("list orgs needing 7d reminder: %v", err)
	}
	due7dIDs := map[string]bool{}
	for _, o := range due7d {
		due7dIDs[o.OrgID] = true
		if o.OrgID == remindDueOrgID && o.OrgName != "Deletion Ledger remind-due" {
			t.Fatalf("7d reminder org name = %q, want %q", o.OrgName, "Deletion Ledger remind-due")
		}
	}
	if !due7dIDs[remindDueOrgID] {
		t.Fatal("org due for 7d reminder was not returned")
	}
	if due7dIDs[notYetDueOrgID] {
		t.Fatal("org not yet due was returned for the 7d reminder")
	}
	if due7dIDs[alreadySentOrgID] {
		t.Fatal("org already reminded (7d) was returned again")
	}

	due1d, err := repo.ListOrgsNeeding1DayReminder(ctx)
	if err != nil {
		t.Fatalf("list orgs needing 1d reminder: %v", err)
	}
	for _, o := range due1d {
		if o.OrgID == remindDueOrgID || o.OrgID == notYetDueOrgID || o.OrgID == alreadySentOrgID {
			t.Fatalf("org %s at <=25 days elapsed was returned for the 1d (29-day) reminder", o.OrgID)
		}
	}

	// MarkReminderSent flips an org out of future 7d selections...
	if err := repo.MarkReminderSent(ctx, remindDueOrgID, "7d"); err != nil {
		t.Fatalf("mark 7d reminder sent: %v", err)
	}
	due7dAfter, err := repo.ListOrgsNeeding7DayReminder(ctx)
	if err != nil {
		t.Fatalf("list orgs needing 7d reminder after marking sent: %v", err)
	}
	for _, o := range due7dAfter {
		if o.OrgID == remindDueOrgID {
			t.Fatal("org still selected for 7d reminder after MarkReminderSent")
		}
	}
	// ...but MarkReminderSent itself only requires "pending deletion", not
	// "actually past the elapsed-day threshold" — the day-threshold gate
	// lives in ListOrgsNeeding*Reminder, not here.
	if err := repo.MarkReminderSent(ctx, notYetDueOrgID, "7d"); err != nil {
		t.Fatalf("mark reminder sent for a pending (if not yet due) org: %v", err)
	}
	if err := repo.MarkReminderSent(ctx, "org-does-not-exist-"+suffix, "7d"); !errors.Is(err, ErrOrganizationNotPendingDeletion) {
		t.Fatalf("mark reminder sent for missing org: err=%v, want ErrOrganizationNotPendingDeletion", err)
	}
	if err := repo.MarkReminderSent(ctx, orgA, "7d"); !errors.Is(err, ErrOrganizationNotPendingDeletion) {
		t.Fatalf("mark reminder sent for a restored (active) org: err=%v, want ErrOrganizationNotPendingDeletion", err)
	}
	if err := repo.MarkReminderSent(ctx, remindDueOrgID, "bogus"); err == nil {
		t.Fatal("mark reminder sent accepted an unsupported reminder kind")
	}
}

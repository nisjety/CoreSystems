package org

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

func TestControlLifecycleDeletionRejectsInvalidInputBeforeDatabaseAccess(t *testing.T) {
	repo := &Repository{}
	for _, revision := range []int64{0, -1, MaxSafeAuthRevision + 1} {
		receipt, applied, err := repo.ReconcileOrganizationDeletion(context.Background(), "org-invalid-revision", revision)
		if err == nil || applied || receipt != nil {
			t.Fatalf("revision %d: receipt=%s applied=%t err=%v", revision, receipt, applied, err)
		}
	}
	if receipt, applied, err := repo.ReconcileOrganizationDeletion(context.Background(), " ", 1); err == nil || applied || receipt != nil {
		t.Fatalf("blank org id: receipt=%s applied=%t err=%v", receipt, applied, err)
	}
}

func TestControlLifecycleDeletionTombstoneDecision(t *testing.T) {
	now := time.Now()
	successReceipt := []byte(`{"success":true,"org_id":"org-decision"}`)
	tests := []struct {
		name             string
		incomingRevision int64
		storedRevision   int64
		completedAt      *time.Time
		receipt          []byte
		wantResume       bool
		wantReceipt      bool
		wantError        error
	}{
		{name: "stale", incomingRevision: 3, storedRevision: 4, wantError: ErrProjectionConflict},
		{name: "id reuse", incomingRevision: 5, storedRevision: 4, wantError: ErrOrganizationDeleted},
		{name: "incomplete exact retry", incomingRevision: 4, storedRevision: 4, wantResume: true},
		{name: "completed exact retry", incomingRevision: 4, storedRevision: 4, completedAt: &now, receipt: successReceipt, wantReceipt: true},
		{name: "corrupt completed checkpoint", incomingRevision: 4, storedRevision: 4, completedAt: &now, receipt: []byte(`{"success":false,"error":"fixture"}`)},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			resume, receipt, err := evaluateDeletionTombstone(
				test.incomingRevision,
				test.storedRevision,
				test.completedAt != nil,
				test.receipt,
			)
			if test.wantError != nil {
				if !errors.Is(err, test.wantError) {
					t.Fatalf("error=%v want %v", err, test.wantError)
				}
				return
			}
			if test.name == "corrupt completed checkpoint" {
				if err == nil {
					t.Fatal("corrupt completed checkpoint was accepted")
				}
				return
			}
			if err != nil || resume != test.wantResume || (len(receipt) > 0) != test.wantReceipt {
				t.Fatalf("resume=%t receipt=%s err=%v", resume, receipt, err)
			}
			if test.wantReceipt && string(receipt) != string(successReceipt) {
				t.Fatalf("receipt=%s want=%s", receipt, successReceipt)
			}
		})
	}
}

// TestControlLifecycleProjectionOrderingAndDeletionRetry is intentionally
// opt-in because it applies the real org-core migrations. The acceptance
// runner must point CONTROL_LIFECYCLE_TEST_DATABASE_URL at a disposable
// Postgres database; this test must never target the live Control database.
func TestControlLifecycleProjectionOrderingAndDeletionRetry(t *testing.T) {
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
		ctx,
		db.Pool,
		dsn,
		"org_lifecycle",
		os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID"),
	); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	repo := NewRepository(db)
	var tombstoneDeletePrivilege, tombstoneRevisionUpdatePrivilege bool
	var tombstoneCheckpointUpdatePrivilege, tombstoneRevisionInsertPrivilege bool
	if err := db.Pool.QueryRow(ctx, `
SELECT has_table_privilege(
  'org_core_app', 'auth_organization_tombstones', 'DELETE'
), has_column_privilege(
  'org_core_app', 'auth_organization_tombstones', 'revision', 'UPDATE'
), has_column_privilege(
  'org_core_app', 'auth_organization_tombstones', 'erasure_completed_at', 'UPDATE'
), has_column_privilege(
  'org_core_app', 'auth_organization_tombstones', 'revision', 'INSERT'
)`).Scan(
		&tombstoneDeletePrivilege,
		&tombstoneRevisionUpdatePrivilege,
		&tombstoneCheckpointUpdatePrivilege,
		&tombstoneRevisionInsertPrivilege,
	); err != nil {
		t.Fatalf("inspect tombstone privileges: %v", err)
	}
	if tombstoneDeletePrivilege || tombstoneRevisionUpdatePrivilege ||
		!tombstoneCheckpointUpdatePrivilege || !tombstoneRevisionInsertPrivilege {
		t.Fatalf(
			"unsafe tombstone privileges: delete=%t revision_update=%t checkpoint_update=%t revision_insert=%t",
			tombstoneDeletePrivilege,
			tombstoneRevisionUpdatePrivilege,
			tombstoneCheckpointUpdatePrivilege,
			tombstoneRevisionInsertPrivilege,
		)
	}
	orgID := "org-lifecycle-retry"
	ownerID := "owner-lifecycle-retry"
	memberID := "member-lifecycle-retry"

	applied, err := repo.ReconcileOrganizationProjection(ctx, Organization{
		ID: orgID, Name: "Revision One", Slug: "revision-one",
	}, ownerID, 1)
	if err != nil || !applied {
		t.Fatalf("apply initial organization projection: applied=%t err=%v", applied, err)
	}

	// An exact retry is a no-op, while a conflicting payload at the same
	// revision fails closed instead of being silently treated as a duplicate.
	applied, err = repo.ReconcileOrganizationProjection(ctx, Organization{
		ID: orgID, Name: "Revision One", Slug: "revision-one",
	}, ownerID, 1)
	if err != nil || applied {
		t.Fatalf("exact organization retry: applied=%t err=%v", applied, err)
	}
	applied, err = repo.ReconcileOrganizationProjection(ctx, Organization{
		ID: orgID, Name: "Conflicting Revision One", Slug: "conflict",
	}, ownerID, 1)
	if !errors.Is(err, ErrProjectionConflict) || applied {
		t.Fatalf("conflicting organization revision: applied=%t err=%v", applied, err)
	}
	applied, err = repo.ReconcileOrganizationProjection(ctx, Organization{
		ID: orgID, Name: "Revision Three", Slug: "revision-three",
	}, ownerID, 3)
	if err != nil || !applied {
		t.Fatalf("apply newer organization revision: applied=%t err=%v", applied, err)
	}
	applied, err = repo.ReconcileOrganizationProjection(ctx, Organization{
		ID: orgID, Name: "Delayed Revision Two", Slug: "revision-two",
	}, ownerID, 2)
	if err != nil || applied {
		t.Fatalf("delayed organization revision: applied=%t err=%v", applied, err)
	}

	var name string
	if err := db.Pool.QueryRow(ctx,
		`SELECT name FROM organizations WHERE id = $1`, orgID,
	).Scan(&name); err != nil {
		t.Fatalf("read projected organization: %v", err)
	}
	if name != "Revision Three" {
		t.Fatalf("projected organization name = %q; want newest revision", name)
	}

	applied, err = repo.ReconcileOrganizationMember(ctx, orgID, memberID, "admin", "upsert", 5)
	if err != nil || !applied {
		t.Fatalf("apply membership revision: applied=%t err=%v", applied, err)
	}
	applied, err = repo.ReconcileOrganizationMember(ctx, orgID, memberID, "admin", "upsert", 5)
	if err != nil || applied {
		t.Fatalf("exact membership retry: applied=%t err=%v", applied, err)
	}
	applied, err = repo.ReconcileOrganizationMember(ctx, orgID, memberID, "viewer", "upsert", 5)
	if !errors.Is(err, ErrProjectionConflict) || applied {
		t.Fatalf("conflicting membership revision: applied=%t err=%v", applied, err)
	}
	applied, err = repo.ReconcileOrganizationMember(ctx, orgID, memberID, "member", "remove", 4)
	if err != nil || applied {
		t.Fatalf("delayed membership removal: applied=%t err=%v", applied, err)
	}
	var role, status string
	if err := db.Pool.QueryRow(ctx,
		`SELECT role, status FROM organization_members WHERE org_id = $1 AND user_id = $2`,
		orgID, memberID,
	).Scan(&role, &status); err != nil {
		t.Fatalf("read projected member: %v", err)
	}
	if role != "admin" || status != "active" {
		t.Fatalf("member after delayed removal = role %q status %q", role, status)
	}

	// Force the erasure procedure to report a semantic failure after the
	// independently committed tombstone. This models a transient dependent
	// service/database failure without touching any real tenant.
	if _, err := db.Pool.Exec(ctx, `
ALTER FUNCTION gdpr_hard_delete_organization(TEXT)
  RENAME TO lifecycle_original_gdpr_hard_delete_organization;
CREATE FUNCTION gdpr_hard_delete_organization(org_id_param TEXT)
RETURNS JSONB
LANGUAGE sql
SECURITY DEFINER
AS $$
  SELECT jsonb_build_object(
    'success', false,
    'error', 'fixture transient erasure failure',
    'org_id', org_id_param
  )
$$;
`); err != nil {
		t.Fatalf("install disposable erasure fault: %v", err)
	}

	if receipt, applied, err := repo.ReconcileOrganizationDeletion(ctx, orgID, 2); !errors.Is(err, ErrProjectionConflict) || applied {
		t.Fatalf("stale organization deletion: applied=%t receipt=%s err=%v", applied, receipt, err)
	}
	if receipt, applied, err := repo.ReconcileOrganizationDeletion(ctx, orgID, 3); !errors.Is(err, ErrProjectionConflict) || applied {
		t.Fatalf("same-revision organization deletion: applied=%t receipt=%s err=%v", applied, receipt, err)
	}

	receipt, applied, err := repo.ReconcileOrganizationDeletion(ctx, orgID, 4)
	if err == nil {
		t.Fatalf("semantic erasure failure was accepted: receipt=%s", receipt)
	}
	if applied {
		t.Fatal("failed erasure reported deletion as applied")
	}
	var tombstones, organizations int
	var tombstoneRevision int64
	var erasureCompleted bool
	if err := db.Pool.QueryRow(ctx,
		`SELECT COUNT(*)::INT, COALESCE(MAX(revision), 0), BOOL_AND(erasure_completed_at IS NOT NULL)
		 FROM auth_organization_tombstones WHERE org_id = $1`, orgID,
	).Scan(&tombstones, &tombstoneRevision, &erasureCompleted); err != nil {
		t.Fatalf("read deletion tombstone after failure: %v", err)
	}
	if err := db.Pool.QueryRow(ctx,
		`SELECT COUNT(*)::INT FROM organizations WHERE id = $1`, orgID,
	).Scan(&organizations); err != nil {
		t.Fatalf("read organization after failed erasure: %v", err)
	}
	if tombstones != 1 || tombstoneRevision != 4 || erasureCompleted || organizations != 1 {
		t.Fatalf("failed erasure state: tombstones=%d revision=%d completed=%t organizations=%d; want 1/4/false/1", tombstones, tombstoneRevision, erasureCompleted, organizations)
	}
	if receipt, applied, err := repo.ReconcileOrganizationDeletion(ctx, orgID, 5); !errors.Is(err, ErrOrganizationDeleted) || applied {
		t.Fatalf("newer deletion after permanent tombstone: applied=%t receipt=%s err=%v", applied, receipt, err)
	}

	if _, err := db.Pool.Exec(ctx, `
DROP FUNCTION gdpr_hard_delete_organization(TEXT);
ALTER FUNCTION lifecycle_original_gdpr_hard_delete_organization(TEXT)
  RENAME TO gdpr_hard_delete_organization;
`); err != nil {
		t.Fatalf("remove disposable erasure fault: %v", err)
	}

	receipt, applied, err = repo.ReconcileOrganizationDeletion(ctx, orgID, 4)
	if err != nil {
		t.Fatalf("retry organization deletion: %v", err)
	}
	if !applied {
		t.Fatal("successful erasure retry was not applied")
	}
	var decoded struct {
		Success bool `json:"success"`
	}
	if err := json.Unmarshal(receipt, &decoded); err != nil || !decoded.Success {
		t.Fatalf("retry deletion receipt = %s err=%v", receipt, err)
	}

	// A second retry is idempotent, and delayed organization/membership
	// deliveries cannot recreate authority after the permanent tombstone.
	if _, err := db.Pool.Exec(ctx, `
ALTER FUNCTION gdpr_hard_delete_organization(TEXT)
  RENAME TO lifecycle_completed_gdpr_hard_delete_organization;
CREATE FUNCTION gdpr_hard_delete_organization(org_id_param TEXT)
RETURNS JSONB
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
BEGIN
  RAISE EXCEPTION 'completed deletion retry invoked erasure';
END
$$;
`); err != nil {
		t.Fatalf("install completed-retry erasure fault: %v", err)
	}
	retryReceipt, retryApplied, err := repo.ReconcileOrganizationDeletion(ctx, orgID, 4)
	if err != nil || retryApplied {
		t.Fatalf("idempotent deletion retry: applied=%t err=%v", retryApplied, err)
	}
	if string(retryReceipt) != string(receipt) {
		t.Fatalf("idempotent deletion receipt changed: first=%s retry=%s", receipt, retryReceipt)
	}
	if _, err := db.Pool.Exec(ctx, `
DROP FUNCTION gdpr_hard_delete_organization(TEXT);
ALTER FUNCTION lifecycle_completed_gdpr_hard_delete_organization(TEXT)
  RENAME TO gdpr_hard_delete_organization;
`); err != nil {
		t.Fatalf("remove completed-retry erasure fault: %v", err)
	}
	if _, applied, err := repo.ReconcileOrganizationDeletion(ctx, orgID, 3); !errors.Is(err, ErrProjectionConflict) || applied {
		t.Fatalf("stale post-delete revision: applied=%t err=%v", applied, err)
	}
	applied, err = repo.ReconcileOrganizationMember(ctx, orgID, memberID, "admin", "upsert", 6)
	if err != nil || applied {
		t.Fatalf("post-delete member delivery: applied=%t err=%v", applied, err)
	}
	applied, err = repo.ReconcileOrganizationProjection(ctx, Organization{
		ID: orgID, Name: "Must Stay Deleted", Slug: "must-stay-deleted",
	}, ownerID, 4)
	if !errors.Is(err, ErrOrganizationDeleted) || applied {
		t.Fatalf("post-delete organization delivery: applied=%t err=%v", applied, err)
	}

	if err := db.Pool.QueryRow(ctx,
		`SELECT COUNT(*)::INT FROM organizations WHERE id = $1`, orgID,
	).Scan(&organizations); err != nil {
		t.Fatalf("read organization after delayed deliveries: %v", err)
	}
	if organizations != 0 {
		t.Fatalf("delayed delivery resurrected %d organization row(s)", organizations)
	}
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO organizations (id, name, plan, status, region, default_locale, metadata)
VALUES ($1, 'Reused Tombstoned ID', 'free', 'active', 'eu', 'nb-NO', '{}'::JSONB)
`, orgID); err == nil {
		t.Fatal("direct SQL insert reused a permanently tombstoned organization id")
	}
}

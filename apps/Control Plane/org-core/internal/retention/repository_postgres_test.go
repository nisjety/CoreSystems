package retention

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
	"github.com/jackc/pgx/v5"
)

// openRetentionLifecycleDB connects to the disposable lifecycle Postgres (the
// fixture the browser site repository test uses) and applies every migration.
func openRetentionLifecycleDB(t *testing.T) (context.Context, *database.DB) {
	t.Helper()
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	t.Cleanup(cancel)
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	t.Cleanup(db.Close)
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}
	return ctx, db
}

func seedRetentionOrganization(t *testing.T, ctx context.Context, db *database.DB, orgID string) {
	t.Helper()
	// Migration 010's deferred constraint trigger requires every live organization
	// to have an active owner by COMMIT, so both rows go in one transaction.
	tx, err := db.Pool.Begin(ctx)
	if err != nil {
		t.Fatalf("begin seed tx for %s: %v", orgID, err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `INSERT INTO organizations (id, name) VALUES ($1, $2)`, orgID, "Retention "+orgID); err != nil {
		t.Fatalf("seed organization %s: %v", orgID, err)
	}
	if _, err := tx.Exec(ctx, `INSERT INTO organization_members (org_id, user_id, role, status) VALUES ($1, $2, 'owner', 'active')`, orgID, "owner-"+orgID); err != nil {
		t.Fatalf("seed owner for %s: %v", orgID, err)
	}
	if err := tx.Commit(ctx); err != nil {
		t.Fatalf("commit seed tx for %s: %v", orgID, err)
	}
}

func hold(id, scope, target string) Hold {
	return Hold{ID: id, Scope: scope, TargetRef: target, HoldRef: "Matter 2026-17"}
}

// TestControlLifecycleRetentionRepositoryUnderRLS proves what EP-9 C1 requires of
// the store: the revision compare-and-set, the window bounds at the database, an
// append-only history, one active hold per target, release, the check answer,
// tenant isolation under RLS, a content-free audit intent per write, and that
// hard-deleting the organisation leaves no row behind.
func TestControlLifecycleRetentionRepositoryUnderRLS(t *testing.T) {
	ctx, db := openRetentionLifecycleDB(t)
	suffix := time.Now().UnixNano()
	orgA := fmt.Sprintf("org-retention-a-%d", suffix)
	orgB := fmt.Sprintf("org-retention-b-%d", suffix)
	seedRetentionOrganization(t, ctx, db, orgA)
	seedRetentionOrganization(t, ctx, db, orgB)
	repo := NewRepository(db)
	actor := Actor{ID: "actor-1", Role: "owner"}

	empty, err := repo.GetPolicy(ctx, orgA)
	if err != nil || empty.Revision != 0 || empty.ThreadDays != nil {
		t.Fatalf("a new organisation keeps everything: %+v err=%v", empty, err)
	}
	days := 90
	first, err := repo.PutPolicy(ctx, orgA, actor, 0, &days)
	if err != nil || first.Revision != 1 || first.ThreadDays == nil || *first.ThreadDays != 90 || first.UpdatedBy != "actor-1" {
		t.Fatalf("first write: %+v err=%v", first, err)
	}
	var conflict *RevisionConflictError
	if _, err := repo.PutPolicy(ctx, orgA, actor, 0, nil); !errors.As(err, &conflict) || conflict.Current != 1 {
		t.Fatalf("a stale write must conflict at revision 1, got %v", err)
	}
	keep, err := repo.PutPolicy(ctx, orgA, actor, 1, nil)
	if err != nil || keep.Revision != 2 || keep.ThreadDays != nil {
		t.Fatalf("setting keep: %+v err=%v", keep, err)
	}

	// The database refuses a window outside 30..3650 even from a writer that is not
	// the service.
	for _, bad := range []int{29, 3651} {
		err := db.WithOrgScope(ctx, orgA, func(tx pgx.Tx) error {
			_, err := tx.Exec(ctx, `UPDATE org_retention_policies SET thread_days = $2 WHERE org_id = $1`, orgA, bad)
			return err
		})
		if err == nil {
			t.Fatalf("a window of %d days must be refused by the table", bad)
		}
	}

	// Holds: one active hold per target; placing it again answers the existing one.
	orgHold, err := repo.PlaceHold(ctx, orgA, actor, hold("lh_"+strings.Repeat("a", 32), ScopeOrg, orgA))
	if err != nil || orgHold.Existed || orgHold.Hold.TargetRef != orgA || orgHold.Hold.ReleasedAt != nil {
		t.Fatalf("org hold: %+v err=%v", orgHold, err)
	}
	threadHold, err := repo.PlaceHold(ctx, orgA, actor, hold("lh_"+strings.Repeat("b", 32), ScopeThread, "thr-1"))
	if err != nil || threadHold.Existed {
		t.Fatalf("thread hold: %+v err=%v", threadHold, err)
	}
	again, err := repo.PlaceHold(ctx, orgA, actor, hold("lh_"+strings.Repeat("c", 32), ScopeThread, "thr-1"))
	if err != nil || !again.Existed || again.Hold.ID != threadHold.Hold.ID {
		t.Fatalf("a second hold on a held thread must answer the first: %+v err=%v", again, err)
	}
	if err := db.WithOrgScope(ctx, orgA, func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx, `INSERT INTO org_legal_holds (id, org_id, scope, target_ref, hold_ref, created_by) VALUES ($1, $2, 'org', 'other-org', 'm', 'x')`, "lh_"+strings.Repeat("d", 32), orgA)
		return err
	}); err == nil {
		t.Fatal("an org hold must target its own organisation")
	}

	check, err := repo.Check(ctx, orgA, []string{"thr-1", "thr-2"})
	if err != nil || !check.OrgHeld || check.Revision != 2 || check.WindowDays != nil || len(check.HeldThreadIDs) != 1 || check.HeldThreadIDs[0] != "thr-1" {
		t.Fatalf("check must report the org hold and the held thread only: %+v err=%v", check, err)
	}

	released, err := repo.ReleaseHold(ctx, orgA, orgHold.Hold.ID, Actor{ID: "compliance-1", Role: "compliance"})
	if err != nil || released.ReleasedAt == nil || released.ReleasedBy == nil || *released.ReleasedBy != "compliance-1" {
		t.Fatalf("release: %+v err=%v", released, err)
	}
	if _, err := repo.ReleaseHold(ctx, orgA, orgHold.Hold.ID, actor); !errors.Is(err, ErrHoldAlreadyReleased) {
		t.Fatalf("a second release must be ErrHoldAlreadyReleased, got %v", err)
	}
	if _, err := repo.ReleaseHold(ctx, orgA, "lh_"+strings.Repeat("e", 32), actor); !errors.Is(err, ErrHoldNotFound) {
		t.Fatalf("an unknown hold must be ErrHoldNotFound, got %v", err)
	}
	if check, err := repo.Check(ctx, orgA, nil); err != nil || check.OrgHeld {
		t.Fatalf("a released org hold no longer holds: %+v err=%v", check, err)
	}
	if replaced, err := repo.PlaceHold(ctx, orgA, actor, hold("lh_"+strings.Repeat("f", 32), ScopeOrg, orgA)); err != nil || replaced.Existed {
		t.Fatalf("a released target can be held again: %+v err=%v", replaced, err)
	}

	// The runtime role can neither rewrite a hold nor delete one, nor touch the
	// history.
	for _, statement := range []string{
		`UPDATE org_legal_holds SET target_ref = 'thr-9' WHERE org_id = $1`,
		`UPDATE org_legal_holds SET hold_ref = 'tampered' WHERE org_id = $1`,
		`DELETE FROM org_legal_holds WHERE org_id = $1`,
		`DELETE FROM org_retention_policies WHERE org_id = $1`,
		`UPDATE org_retention_history SET actor_id = 'tampered' WHERE org_id = $1`,
		`DELETE FROM org_retention_history WHERE org_id = $1`,
		`UPDATE org_retention_audit_outbox SET payload = '{}'::jsonb WHERE org_id = $1`,
		`DELETE FROM org_retention_audit_outbox WHERE org_id = $1`,
	} {
		err := db.WithOrgScope(ctx, orgA, func(tx pgx.Tx) error {
			_, err := tx.Exec(ctx, statement, orgA)
			return err
		})
		if err == nil {
			t.Fatalf("org_core_app must not be able to run %q", statement)
		}
	}

	var history int
	if err := db.Pool.QueryRow(ctx, `
SELECT count(*) FROM org_retention_history WHERE org_id = $1`, orgA).Scan(&history); err != nil || history != 6 {
		t.Fatalf("history rows = %d err=%v; want 2 policy writes, 3 placements, 1 release", history, err)
	}
	var audits int
	var leaked int
	if err := db.Pool.QueryRow(ctx, `
SELECT count(*), count(*) FILTER (WHERE payload::text LIKE '%Matter 2026-17%')
FROM org_retention_audit_outbox WHERE org_id = $1 AND subject = $2`, orgA, AuditSubject).Scan(&audits, &leaked); err != nil || audits != 6 || leaked != 0 {
		t.Fatalf("audit intents = %d (hold reference in %d) err=%v; want 6 with no reference", audits, leaked, err)
	}

	// Tenant isolation: B's scope reads and writes none of A's rows.
	if other, err := repo.Check(ctx, orgB, []string{"thr-1"}); err != nil || other.OrgHeld || len(other.HeldThreadIDs) != 0 || other.Revision != 0 {
		t.Fatalf("organisation B must not see A's holds or policy: %+v err=%v", other, err)
	}
	for _, table := range []string{"org_retention_policies", "org_legal_holds", "org_retention_history", "org_retention_audit_outbox"} {
		var visible int
		err := db.WithOrgScope(ctx, orgB, func(tx pgx.Tx) error {
			return tx.QueryRow(ctx, fmt.Sprintf(`SELECT count(*) FROM %s WHERE org_id = $1`, table), orgA).Scan(&visible)
		})
		if err != nil || visible != 0 {
			t.Fatalf("RLS leaked %d %s rows of A into B's scope (err=%v)", visible, table, err)
		}
	}
	if err := db.WithOrgScope(ctx, orgB, func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx, `INSERT INTO org_legal_holds (id, org_id, scope, target_ref, hold_ref, created_by) VALUES ($1, $2, 'thread', 'thr-x', 'm', 'x')`, "lh_"+strings.Repeat("9", 32), orgA)
		return err
	}); err == nil {
		t.Fatal("a hold for A under B's scope must be refused by RLS")
	}

	// A write for an organisation that does not exist is a typed error, and the
	// check route treats a missing organisation as not found, never as "no hold".
	missing := fmt.Sprintf("org-retention-missing-%d", suffix)
	if _, err := repo.PutPolicy(ctx, missing, actor, 0, &days); !errors.Is(err, ErrOrganizationNotFound) {
		t.Fatalf("a write for a missing organisation must be ErrOrganizationNotFound, got %v", err)
	}
	if _, err := repo.Check(ctx, missing, nil); !errors.Is(err, ErrOrganizationNotFound) {
		t.Fatalf("a check for a missing organisation must be ErrOrganizationNotFound, got %v", err)
	}

	// Erasure: hard-deleting the organisation takes every row with it through the
	// foreign keys, without gdpr_hard_delete_organization being redefined. The audit
	// outbox has no foreign key, as the other outboxes, so an erasure's audit trail
	// is not erased with it.
	var receipt []byte
	if err := db.Pool.QueryRow(ctx, `SELECT gdpr_hard_delete_organization($1)::text`, orgA).Scan(&receipt); err != nil {
		t.Fatalf("hard delete: %v", err)
	}
	var outcome struct {
		Success bool `json:"success"`
	}
	if err := json.Unmarshal(receipt, &outcome); err != nil || !outcome.Success {
		t.Fatalf("hard delete did not succeed: %s (%v)", receipt, err)
	}
	for _, table := range []string{"org_retention_policies", "org_legal_holds", "org_retention_history"} {
		var left int
		if err := db.Pool.QueryRow(ctx, fmt.Sprintf(`SELECT count(*) FROM %s WHERE org_id = $1`, table), orgA).Scan(&left); err != nil || left != 0 {
			t.Fatalf("%d %s rows survived the hard delete of the organisation (err=%v)", left, table, err)
		}
	}
}

// TestControlLifecycleRetentionGrantsAreLeastPrivilege pins what org_core_app may
// do on each table migration 031 creates. 009's default privileges would otherwise
// give it all four on every one of them (D1-751).
func TestControlLifecycleRetentionGrantsAreLeastPrivilege(t *testing.T) {
	ctx, db := openRetentionLifecycleDB(t)
	want := map[string]string{
		"org_retention_policies":     "SELECT,INSERT",
		"org_legal_holds":            "SELECT,INSERT",
		"org_retention_history":      "SELECT,INSERT",
		"org_retention_audit_outbox": "SELECT,INSERT",
	}
	for table, need := range want {
		var canSelect, canInsert, canUpdate, canDelete bool
		if err := db.Pool.QueryRow(ctx, `
SELECT has_table_privilege('org_core_app', $1::text, 'SELECT'),
       has_table_privilege('org_core_app', $1::text, 'INSERT'),
       has_table_privilege('org_core_app', $1::text, 'UPDATE'),
       has_table_privilege('org_core_app', $1::text, 'DELETE')`, table).Scan(&canSelect, &canInsert, &canUpdate, &canDelete); err != nil {
			t.Fatalf("inspect org_core_app privileges on %s: %v", table, err)
		}
		var got []string
		for name, held := range map[string]bool{"SELECT": canSelect, "INSERT": canInsert, "UPDATE": canUpdate, "DELETE": canDelete} {
			if held {
				got = append(got, name)
			}
		}
		if canUpdate || canDelete || !canSelect || !canInsert {
			t.Errorf("org_core_app holds %v on %s; the code needs %s (and column UPDATE where stated)", got, table, need)
		}
	}
	columnUpdate := map[string]map[string]bool{
		"org_retention_policies": {"thread_days": true, "revision": true, "updated_by": true, "updated_at": true, "org_id": false},
		"org_legal_holds":        {"released_by": true, "released_at": true, "scope": false, "target_ref": false, "hold_ref": false, "org_id": false, "created_by": false},
	}
	for table, columns := range columnUpdate {
		for column, need := range columns {
			var held bool
			if err := db.Pool.QueryRow(ctx, `SELECT has_column_privilege('org_core_app', $1::text, $2::text, 'UPDATE')`, table, column).Scan(&held); err != nil {
				t.Fatalf("inspect column privilege %s.%s: %v", table, column, err)
			}
			if held != need {
				t.Errorf("org_core_app UPDATE on %s.%s = %v, want %v", table, column, held, need)
			}
		}
	}
}

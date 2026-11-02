package browsersites

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
	"github.com/jackc/pgx/v5"
)

func seedBrowserSiteOrganization(t *testing.T, ctx context.Context, db *database.DB, orgID string) {
	t.Helper()
	// Migration 010's deferred constraint trigger requires every live organization
	// to have an active owner by COMMIT, so both rows go in one transaction.
	tx, err := db.Pool.Begin(ctx)
	if err != nil {
		t.Fatalf("begin seed tx for %s: %v", orgID, err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `INSERT INTO organizations (id, name) VALUES ($1, $2)`, orgID, "Browser sites "+orgID); err != nil {
		t.Fatalf("seed organization %s: %v", orgID, err)
	}
	if _, err := tx.Exec(ctx, `INSERT INTO organization_members (org_id, user_id, role, status) VALUES ($1, $2, 'owner', 'active')`, orgID, "owner-"+orgID); err != nil {
		t.Fatalf("seed owner for %s: %v", orgID, err)
	}
	if err := tx.Commit(ctx); err != nil {
		t.Fatalf("commit seed tx for %s: %v", orgID, err)
	}
}

func browserSitePut(expected int64, mode string, rules ...Rule) PutRequest {
	return PutRequest{ExpectedRevision: &expected, Mode: mode, Rules: rules}
}

// TestControlLifecycleBrowserSitesRepositoryUnderRLS runs against the disposable
// lifecycle Postgres (the fixture the model policy repository test uses) and proves
// what WS21-S3 requires of the store: tenant isolation under RLS, the revision
// compare-and-set, a history row for every policy and member write, an append-only
// history, the per-member choice limit, and that hard-deleting the organisation
// leaves no row behind.
func TestControlLifecycleBrowserSitesRepositoryUnderRLS(t *testing.T) {
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
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	suffix := time.Now().UnixNano()
	orgA := fmt.Sprintf("org-browser-sites-a-%d", suffix)
	orgB := fmt.Sprintf("org-browser-sites-b-%d", suffix)
	seedBrowserSiteOrganization(t, ctx, db, orgA)
	seedBrowserSiteOrganization(t, ctx, db, orgB)
	repo := NewRepository(db)

	empty, err := repo.Get(ctx, orgA)
	if err != nil || empty.Revision != 0 || empty.Mode != ModeOpen || len(empty.Rules) != 0 {
		t.Fatalf("a new organisation has the open default: %+v err=%v", empty, err)
	}

	first, err := repo.Put(ctx, orgA, "actor-1", 0, browserSitePut(0, ModeAllowlist, Rule{Host: "shop.example.com", Rule: RuleAct, Note: "checkout"}))
	if err != nil || first.Revision != 1 || first.UpdatedBy != "actor-1" || len(first.Rules) != 1 || first.Rules[0].Note != "checkout" {
		t.Fatalf("first write: %+v err=%v", first, err)
	}

	var conflict *RevisionConflictError
	if _, err := repo.Put(ctx, orgA, "actor-2", 0, browserSitePut(0, ModeOpen)); !errors.As(err, &conflict) || conflict.Current != 1 {
		t.Fatalf("a stale write must be a RevisionConflictError at revision 1, got %v", err)
	}
	if unchanged, err := repo.Get(ctx, orgA); err != nil || unchanged.Revision != 1 || unchanged.Mode != ModeAllowlist {
		t.Fatalf("a refused write must change nothing: %+v err=%v", unchanged, err)
	}

	if _, err := repo.Put(ctx, orgA, "actor-2", 1, browserSitePut(1, ModeOpen, Rule{Host: "blocked.example.com", Rule: RuleBlock})); err != nil {
		t.Fatalf("second write: %v", err)
	}

	// The policy history holds the whole policy before and after, one row per
	// policy write.
	var policyRows int
	var beforeJSON, afterJSON []byte
	if err := db.Pool.QueryRow(ctx, `SELECT count(*) FROM org_browser_site_history WHERE org_id = $1 AND change_kind = 'policy'`, orgA).Scan(&policyRows); err != nil || policyRows != 2 {
		t.Fatalf("policy history rows = %d err=%v; want 2", policyRows, err)
	}
	if err := db.Pool.QueryRow(ctx, `
SELECT before_set, after_set FROM org_browser_site_history WHERE org_id = $1 AND revision = 2 AND actor_id = 'actor-2' AND change_kind = 'policy'`, orgA).Scan(&beforeJSON, &afterJSON); err != nil {
		t.Fatalf("read revision 2 history: %v", err)
	}
	var before, after snapshot
	if err := json.Unmarshal(beforeJSON, &before); err != nil || before.Mode != ModeAllowlist || len(before.Rules) != 1 {
		t.Fatalf("history before_set must hold revision 1: %s err=%v", beforeJSON, err)
	}
	if err := json.Unmarshal(afterJSON, &after); err != nil || after.Mode != ModeOpen || after.Rules[0].Rule != RuleBlock {
		t.Fatalf("history after_set must hold revision 2: %s err=%v", afterJSON, err)
	}

	// A member's choice is checked against the current revision, recorded in the
	// history with the member as actor, and visible to the resolver.
	if _, err := repo.PutPref(ctx, orgA, "member-1", 2, Pref{Host: "open.example.com", Level: LevelAct, ExpiresAt: time.Now().Add(time.Hour)}); err != nil {
		t.Fatalf("member choice at the current revision: %v", err)
	}
	var currentConflict *RevisionConflictError
	if _, err := repo.PutPref(ctx, orgA, "member-1", 0, Pref{Host: "open.example.com", Level: LevelRead, ExpiresAt: time.Now().Add(time.Hour)}); !errors.As(err, &currentConflict) || currentConflict.Current != 2 {
		t.Fatalf("a choice against a superseded revision must conflict at 2, got %v", err)
	}
	var prefHistory int
	if err := db.Pool.QueryRow(ctx, `
SELECT count(*) FROM org_browser_site_history WHERE org_id = $1 AND change_kind = 'user_pref' AND subject_user_id = 'member-1' AND actor_id = 'member-1'`, orgA).Scan(&prefHistory); err != nil || prefHistory != 1 {
		t.Fatalf("a member choice must leave one history row by the member: %d err=%v", prefHistory, err)
	}
	set, prefs, err := repo.Snapshot(ctx, orgA, "member-1", []string{"open.example.com", "blocked.example.com"})
	if err != nil || set.Revision != 2 || prefs["open.example.com"].Level != LevelAct || len(prefs) != 1 {
		t.Fatalf("snapshot must carry the policy and the member's live choice: set=%+v prefs=%+v err=%v", set, prefs, err)
	}

	// Tenant isolation: B's scope reads and writes none of A's rows.
	other, err := repo.Get(ctx, orgB)
	if err != nil || other.Revision != 0 || len(other.Rules) != 0 {
		t.Fatalf("organisation B must not see A's policy: %+v err=%v", other, err)
	}
	for _, table := range []string{"org_browser_site_policy", "org_browser_site_rules", "user_browser_site_prefs", "org_browser_site_history"} {
		var visible int
		err := db.WithOrgScope(ctx, orgB, func(tx pgx.Tx) error {
			return tx.QueryRow(ctx, fmt.Sprintf(`SELECT count(*) FROM %s WHERE org_id = $1`, table), orgA).Scan(&visible)
		})
		if err != nil || visible != 0 {
			t.Fatalf("RLS leaked %d %s rows of A into B's scope (err=%v)", visible, table, err)
		}
	}
	crossWrite := db.WithOrgScope(ctx, orgB, func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx, `INSERT INTO org_browser_site_rules (org_id, host, rule) VALUES ($1, 'evil.example.com', 'act')`, orgA)
		return err
	})
	if crossWrite == nil {
		t.Fatal("a rule for A under B's scope must be refused by RLS")
	}

	// The history is append-only for the runtime role.
	for _, statement := range []string{
		`UPDATE org_browser_site_history SET actor_id = 'tampered' WHERE org_id = $1`,
		`DELETE FROM org_browser_site_history WHERE org_id = $1`,
	} {
		err := db.WithOrgScope(ctx, orgA, func(tx pgx.Tx) error {
			_, err := tx.Exec(ctx, statement, orgA)
			return err
		})
		if err == nil {
			t.Fatalf("org_core_app must not be able to run %q", statement)
		}
	}

	// Deleting a choice writes a history row and a second delete is not found.
	if err := repo.DeletePref(ctx, orgA, "member-1", "open.example.com"); err != nil {
		t.Fatalf("delete choice: %v", err)
	}
	if err := repo.DeletePref(ctx, orgA, "member-1", "open.example.com"); !errors.Is(err, ErrPrefNotFound) {
		t.Fatalf("a second delete must be ErrPrefNotFound, got %v", err)
	}

	// The per-member limit holds at MaxPrefsPerUser.
	for i := 0; i < MaxPrefsPerUser; i++ {
		host := fmt.Sprintf("site%03d.example.com", i)
		if _, err := repo.PutPref(ctx, orgA, "member-2", 2, Pref{Host: host, Level: LevelRead, ExpiresAt: time.Now().Add(time.Hour)}); err != nil {
			t.Fatalf("choice %d: %v", i, err)
		}
	}
	if _, err := repo.PutPref(ctx, orgA, "member-2", 2, Pref{Host: "over.example.com", Level: LevelRead, ExpiresAt: time.Now().Add(time.Hour)}); !errors.Is(err, ErrPrefLimitReached) {
		t.Fatalf("the choice after the limit must be ErrPrefLimitReached, got %v", err)
	}

	// A write for an organisation that does not exist is a typed error.
	if _, err := repo.Put(ctx, "org-browser-sites-missing-"+fmt.Sprint(suffix), "actor-1", 0, browserSitePut(0, ModeOpen)); !errors.Is(err, ErrOrganizationNotFound) {
		t.Fatalf("a write for a missing organisation must be ErrOrganizationNotFound, got %v", err)
	}

	// Erasure: hard-deleting the organisation takes every row with it, through the
	// foreign keys, without gdpr_hard_delete_organization being redefined.
	var receipt []byte
	if err := db.Pool.QueryRow(ctx, `SELECT gdpr_hard_delete_organization($1)::text`, orgA).Scan(&receipt); err != nil {
		t.Fatalf("hard delete: %v", err)
	}
	var outcome struct {
		Success bool   `json:"success"`
		Error   string `json:"error"`
	}
	if err := json.Unmarshal(receipt, &outcome); err != nil || !outcome.Success {
		t.Fatalf("hard delete did not succeed: %s (%v)", receipt, err)
	}
	for _, table := range []string{"org_browser_site_policy", "org_browser_site_rules", "user_browser_site_prefs", "org_browser_site_history"} {
		var left int
		if err := db.Pool.QueryRow(ctx, fmt.Sprintf(`SELECT count(*) FROM %s WHERE org_id = $1`, table), orgA).Scan(&left); err != nil || left != 0 {
			t.Fatalf("%d %s rows survived the hard delete of the organisation (err=%v)", left, table, err)
		}
	}
}

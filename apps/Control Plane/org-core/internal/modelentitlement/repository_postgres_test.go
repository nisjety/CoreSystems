package modelentitlement

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
	"github.com/jackc/pgx/v5"
)

func seedOrganization(t *testing.T, ctx context.Context, db *database.DB, orgID string) {
	t.Helper()
	// Migration 010's deferred constraint trigger requires every live
	// organization to have an active owner by COMMIT, so both rows go in one
	// transaction.
	tx, err := db.Pool.Begin(ctx)
	if err != nil {
		t.Fatalf("begin seed tx for %s: %v", orgID, err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `INSERT INTO organizations (id, name) VALUES ($1, $2)`, orgID, "Model policy "+orgID); err != nil {
		t.Fatalf("seed organization %s: %v", orgID, err)
	}
	if _, err := tx.Exec(ctx, `INSERT INTO organization_members (org_id, user_id, role, status) VALUES ($1, $2, 'owner', 'active')`, orgID, "owner-"+orgID); err != nil {
		t.Fatalf("seed owner for %s: %v", orgID, err)
	}
	if err := tx.Commit(ctx); err != nil {
		t.Fatalf("commit seed tx for %s: %v", orgID, err)
	}
}

func putRequest(expected int64, allow ...string) PutRequest {
	normalised, err := ValidatePut(PutRequest{
		ExpectedRevision: &expected,
		OrgDefault:       &Policy{AllowMode: AllowModeListed, Allow: allow, MaxEffort: "high", Defaults: map[string]SurfaceDefault{"chat": {Model: allow[0], Effort: "medium"}}},
		Roles:            map[string]Policy{"viewer": {AllowMode: AllowModeListed, Allow: allow[:1]}},
	})
	if err != nil {
		panic(err)
	}
	return normalised
}

// TestControlLifecycleModelEntitlementRepositoryUnderRLS runs against the
// disposable lifecycle Postgres (the same fixture the rbac repository test
// uses) and proves what the plan's S2 row requires of the store: tenant
// isolation under RLS, the revision compare-and-set, a history row written in
// the same transaction, an append-only history, and that hard-deleting the
// organisation leaves no row behind.
func TestControlLifecycleModelEntitlementRepositoryUnderRLS(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
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
	orgA := fmt.Sprintf("org-model-policy-a-%d", suffix)
	orgB := fmt.Sprintf("org-model-policy-b-%d", suffix)
	seedOrganization(t, ctx, db, orgA)
	seedOrganization(t, ctx, db, orgB)
	repo := NewRepository(db)

	empty, err := repo.Get(ctx, orgA)
	if err != nil || empty.Revision != 0 || empty.OrgDefault != nil || len(empty.Roles) != 0 {
		t.Fatalf("a new organisation has no policy: %+v err=%v", empty, err)
	}

	first, err := repo.Put(ctx, orgA, "actor-1", 0, putRequest(0, "gpt-5-mini", "provider:anthropic"))
	if err != nil {
		t.Fatalf("first write: %v", err)
	}
	if first.Revision != 1 || first.UpdatedBy != "actor-1" || first.OrgDefault == nil || first.OrgDefault.MaxEffort != "high" ||
		first.OrgDefault.Defaults["chat"].Model != "gpt-5-mini" || len(first.Roles["viewer"].Allow) != 1 {
		t.Fatalf("first write read back wrong: %+v", first)
	}

	// A stale writer is refused with the revision it must re-read.
	var conflict *RevisionConflictError
	if _, err := repo.Put(ctx, orgA, "actor-2", 0, putRequest(0, "o4-mini")); !errors.As(err, &conflict) || conflict.Current != 1 {
		t.Fatalf("a stale write must be a RevisionConflictError at revision 1, got %v", err)
	}
	if unchanged, err := repo.Get(ctx, orgA); err != nil || unchanged.Revision != 1 || unchanged.UpdatedBy != "actor-1" {
		t.Fatalf("a refused write must change nothing: %+v err=%v", unchanged, err)
	}

	second, err := repo.Put(ctx, orgA, "actor-2", 1, putRequest(1, "o4-mini"))
	if err != nil || second.Revision != 2 || second.OrgDefault.Allow[0] != "o4-mini" {
		t.Fatalf("second write: %+v err=%v", second, err)
	}

	// The history rows are written in the same transaction, one per revision,
	// with the whole set before and after.
	var historyRows int
	var beforeJSON, afterJSON []byte
	if err := db.Pool.QueryRow(ctx, `SELECT count(*) FROM org_model_policy_history WHERE org_id = $1`, orgA).Scan(&historyRows); err != nil || historyRows != 2 {
		t.Fatalf("history rows = %d err=%v; want 2", historyRows, err)
	}
	if err := db.Pool.QueryRow(ctx, `
SELECT before_set, after_set FROM org_model_policy_history WHERE org_id = $1 AND revision = 2 AND actor_id = 'actor-2'`, orgA).Scan(&beforeJSON, &afterJSON); err != nil {
		t.Fatalf("read revision 2 history: %v", err)
	}
	var before, after snapshot
	if err := json.Unmarshal(beforeJSON, &before); err != nil || before.OrgDefault == nil || before.OrgDefault.Allow[0] != "gpt-5-mini" {
		t.Fatalf("history before_set must hold revision 1: %s err=%v", beforeJSON, err)
	}
	if err := json.Unmarshal(afterJSON, &after); err != nil || after.OrgDefault == nil || after.OrgDefault.Allow[0] != "o4-mini" {
		t.Fatalf("history after_set must hold revision 2: %s err=%v", afterJSON, err)
	}

	// Tenant isolation: B's scope neither reads nor writes A's rows, and a scope
	// that names nobody sees nothing.
	other, err := repo.Get(ctx, orgB)
	if err != nil || other.Revision != 0 || other.OrgDefault != nil {
		t.Fatalf("organisation B must not see A's policy: %+v err=%v", other, err)
	}
	for _, table := range []string{"org_model_policies", "org_model_policy_revisions", "org_model_policy_history"} {
		var visible int
		err := db.WithOrgScope(ctx, orgB, func(tx pgx.Tx) error {
			return tx.QueryRow(ctx, fmt.Sprintf(`SELECT count(*) FROM %s WHERE org_id = $1`, table), orgA).Scan(&visible)
		})
		if err != nil || visible != 0 {
			t.Fatalf("RLS leaked %d %s rows of A into B's scope (err=%v)", visible, table, err)
		}
	}
	crossWrite := db.WithOrgScope(ctx, orgB, func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx, `
INSERT INTO org_model_policies (org_id, scope_kind, scope_value, allow_mode) VALUES ($1, 'org', '', 'all')`, orgA)
		return err
	})
	if crossWrite == nil {
		t.Fatal("a write for A under B's scope must be refused by RLS")
	}

	// The history is append-only for the runtime role.
	for _, statement := range []string{
		`UPDATE org_model_policy_history SET actor_id = 'tampered' WHERE org_id = $1`,
		`DELETE FROM org_model_policy_history WHERE org_id = $1`,
	} {
		err := db.WithOrgScope(ctx, orgA, func(tx pgx.Tx) error {
			_, err := tx.Exec(ctx, statement, orgA)
			return err
		})
		if err == nil {
			t.Fatalf("org_core_app must not be able to run %q", statement)
		}
	}

	// A write for an organisation that does not exist is a typed error.
	if _, err := repo.Put(ctx, "org-model-policy-missing-"+fmt.Sprint(suffix), "actor-1", 0, putRequest(0, "gpt-5-mini")); !errors.Is(err, ErrOrganizationNotFound) {
		t.Fatalf("a write for a missing organisation must be ErrOrganizationNotFound, got %v", err)
	}

	// Two writers racing on the same revision: exactly one wins.
	var wins, conflicts int
	var mu sync.Mutex
	var group sync.WaitGroup
	for index := 0; index < 4; index++ {
		group.Add(1)
		go func(index int) {
			defer group.Done()
			_, err := repo.Put(ctx, orgA, fmt.Sprintf("racer-%d", index), 2, putRequest(2, fmt.Sprintf("race-model-%d", index)))
			mu.Lock()
			defer mu.Unlock()
			var lost *RevisionConflictError
			switch {
			case err == nil:
				wins++
			case errors.As(err, &lost):
				conflicts++
			default:
				t.Errorf("racer %d: %v", index, err)
			}
		}(index)
	}
	group.Wait()
	if wins != 1 || conflicts != 3 {
		t.Fatalf("racing writers: %d won, %d conflicted; want exactly one winner", wins, conflicts)
	}
	if final, err := repo.Get(ctx, orgA); err != nil || final.Revision != 3 {
		t.Fatalf("after the race the revision must be 3: %+v err=%v", final, err)
	}

	// Erasure: hard-deleting the organisation takes every policy row with it,
	// through the foreign keys, without gdpr_hard_delete_organization being
	// redefined.
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
	for _, table := range []string{"org_model_policies", "org_model_policy_revisions", "org_model_policy_history"} {
		var left int
		if err := db.Pool.QueryRow(ctx, fmt.Sprintf(`SELECT count(*) FROM %s WHERE org_id = $1`, table), orgA).Scan(&left); err != nil || left != 0 {
			t.Fatalf("%d %s rows survived the hard delete of the organisation (err=%v)", left, table, err)
		}
	}
}

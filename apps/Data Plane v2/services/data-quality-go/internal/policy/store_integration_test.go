package policy

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/dbtest"
	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// These tests run against Postgres 16 with the real migrations (MIGRATED_TEST_DATABASE_URL, see
// internal/dbtest): the real org_processing_policy table with its CHECK constraints and row-level
// security, the real admin_audit_log, and the real orgscope helper, which drops to the
// NOBYPASSRLS role dataplane_app. They skip locally without the variable and FAIL under
// REQUIRE_CONTRACT (set by the postgres-contract-tests job).

func uniqueOrg(prefix string) string {
	return prefix + "-" + strings.ReplaceAll(uuid.NewString(), "-", "")[:12]
}

func forgetOrgs(t *testing.T, pool *pgxpool.Pool, orgs ...string) {
	t.Helper()
	t.Cleanup(func() {
		ctx := context.Background()
		_, _ = pool.Exec(ctx, "DELETE FROM org_processing_policy WHERE org_id = ANY($1)", orgs)
		_, _ = pool.Exec(ctx, "DELETE FROM admin_audit_log WHERE org_id = ANY($1)", orgs)
	})
}

var operator = Actor{ID: "corpus-seeder", Type: "service", RequestID: "req-test-1"}

func offInput() Input {
	reason := "eval baseline organization"
	return Input{ContextualRetrieval: Off, GraphExtraction: Inherit, BudgetMode: BudgetInherit, Reason: &reason}
}

func mustValid(t *testing.T, in Input) Input {
	t.Helper()
	out, err := in.Validate()
	if err != nil {
		t.Fatalf("test input is not valid: %v", err)
	}
	return out
}

type auditRow struct {
	ID        int64
	Actor     string
	Action    string
	Kind      string
	Target    string
	RequestID *string
	Outcome   string
	Payload   map[string]any
}

func auditRows(t *testing.T, pool *pgxpool.Pool, org string) []auditRow {
	t.Helper()
	rows, err := pool.Query(context.Background(),
		`SELECT audit_id, actor, action, target_kind, target_id, request_id, outcome, payload
		   FROM admin_audit_log WHERE org_id = $1 ORDER BY audit_id`, org)
	if err != nil {
		t.Fatalf("read the audit log: %v", err)
	}
	defer rows.Close()
	var out []auditRow
	for rows.Next() {
		var row auditRow
		var payload []byte
		if err := rows.Scan(&row.ID, &row.Actor, &row.Action, &row.Kind, &row.Target, &row.RequestID, &row.Outcome, &payload); err != nil {
			t.Fatal(err)
		}
		if err := json.Unmarshal(payload, &row.Payload); err != nil {
			t.Fatalf("audit payload is not JSON: %v", err)
		}
		out = append(out, row)
	}
	if err := rows.Err(); err != nil {
		t.Fatal(err)
	}
	return out
}

// The policy table is isolated per organization for the runtime role: through the store, and
// through the role itself. A superuser pool would see both rows and prove nothing, so every
// assertion about what is hidden goes through the real scope.
func TestPolicyRowsAreIsolatedPerOrganizationForTheRuntimeRole(t *testing.T) {
	pool := dbtest.Pool(t)
	store := NewStore(pool)
	orgA, orgB := uniqueOrg("rls-a"), uniqueOrg("rls-b")
	forgetOrgs(t, pool, orgA, orgB)
	ctx := context.Background()

	if _, err := store.Put(ctx, orgA, mustValid(t, offInput()), operator); err != nil {
		t.Fatalf("Put(A): %v", err)
	}
	if _, err := store.Put(ctx, orgB, mustValid(t, Input{ContextualRetrieval: On, GraphExtraction: Inherit, BudgetMode: BudgetInherit}), operator); err != nil {
		t.Fatalf("Put(B): %v", err)
	}

	gotA, err := store.Get(ctx, orgA)
	if err != nil || !gotA.Exists || gotA.ContextualRetrieval != Off || gotA.UpdatedBy != "corpus-seeder" {
		t.Fatalf("Get(A) = %+v, %v", gotA, err)
	}
	gotB, err := store.Get(ctx, orgB)
	if err != nil || !gotB.Exists || gotB.ContextualRetrieval != On {
		t.Fatalf("Get(B) = %+v, %v", gotB, err)
	}
	// An organization without a row has the default, and says it has no row.
	gotNone, err := store.Get(ctx, uniqueOrg("rls-none"))
	if err != nil || gotNone.Exists || gotNone.ContextualRetrieval != Inherit {
		t.Fatalf("Get(none) = %+v, %v", gotNone, err)
	}

	// The unscoped superuser sees both rows: what hides one is row-level security alone.
	if n := dbtest.Int(t, pool, "SELECT count(*) FROM org_processing_policy WHERE org_id = ANY($1)", []string{orgA, orgB}); n != 2 {
		t.Fatalf("the superuser sees %d rows, want 2", n)
	}

	// As the runtime role scoped to B: A's row is invisible even to an explicit predicate and
	// cannot be written, updated or deleted.
	err = orgscope.WithOrgScope(ctx, pool, orgB, func(tx pgx.Tx) error {
		var role string
		if err := tx.QueryRow(ctx, "SELECT current_user::text").Scan(&role); err != nil {
			return err
		}
		if role != "dataplane_app" {
			return fmt.Errorf("the scope runs as %q, want dataplane_app (an inert RLS proves nothing)", role)
		}
		var visible int
		if err := tx.QueryRow(ctx, "SELECT count(*) FROM org_processing_policy WHERE org_id = $1", orgA).Scan(&visible); err != nil {
			return err
		}
		if visible != 0 {
			return fmt.Errorf("org B's scope sees %d of org A's rows", visible)
		}
		for _, statement := range []string{
			"UPDATE org_processing_policy SET contextual_retrieval = 'on', reason = NULL WHERE org_id = $1",
			"DELETE FROM org_processing_policy WHERE org_id = $1",
		} {
			tag, err := tx.Exec(ctx, statement, orgA)
			if err != nil {
				return err
			}
			if tag.RowsAffected() != 0 {
				return fmt.Errorf("%q affected org A's row from org B's scope", statement)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	// And a write that names A from B's scope is refused by the policy's WITH CHECK.
	err = orgscope.WithOrgScope(ctx, pool, orgB, func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx,
			"INSERT INTO org_processing_policy (org_id, contextual_retrieval, updated_by) VALUES ($1, 'on', 'intruder')", orgA)
		return err
	})
	if err == nil {
		t.Fatal("org B's scope inserted a row for org A")
	}
	// A's row is exactly as it was.
	if after, _ := store.Get(ctx, orgA); after.ContextualRetrieval != Off || after.UpdatedBy != "corpus-seeder" {
		t.Fatalf("org A's policy changed: %+v", after)
	}
}

// A policy change and its audit row are one transaction, and the audit row says who, what and
// what it replaced.
func TestAPolicyChangeIsAuditedInTheSameTransaction(t *testing.T) {
	// Exercises the parts of the policy that nothing enforces yet, as they will be stored once graph-index
	// and the budget gate read them: the store itself does not care.
	enforceEverything(t)
	pool := dbtest.Pool(t)
	store := NewStore(pool)
	org := uniqueOrg("audit")
	forgetOrgs(t, pool, org)
	ctx := context.Background()

	first, err := store.Put(ctx, org, mustValid(t, offInput()), operator)
	if err != nil || first.Action != "set" || first.Before.Exists || !first.After.Exists {
		t.Fatalf("first Put = %+v, %v", first, err)
	}
	second, err := store.Put(ctx, org, mustValid(t, Input{
		ContextualRetrieval: On, GraphExtraction: Inherit, BudgetMode: BudgetAlertOnly,
	}), Actor{ID: "second-operator", Type: "service", RequestID: "req-test-2"})
	if err != nil || second.Before.ContextualRetrieval != Off {
		t.Fatalf("second Put = %+v, %v", second, err)
	}
	reset, err := store.Put(ctx, org, mustValid(t, Input{ContextualRetrieval: Inherit, GraphExtraction: Inherit, BudgetMode: BudgetInherit}), operator)
	if err != nil || reset.Action != "reset" || reset.After.Exists {
		t.Fatalf("reset = %+v, %v", reset, err)
	}

	rows := auditRows(t, pool, org)
	if len(rows) != 3 {
		t.Fatalf("audit rows = %d, want 3 (set, set, reset)", len(rows))
	}
	wantActions := []string{"org_processing_policy.set", "org_processing_policy.set", "org_processing_policy.reset"}
	for i, row := range rows {
		if row.Action != wantActions[i] || row.Kind != "org_processing_policy" || row.Target != org || row.Outcome != "ok" {
			t.Fatalf("audit row %d = %+v", i, row)
		}
	}
	if rows[0].Actor != "corpus-seeder" || rows[1].Actor != "second-operator" {
		t.Fatalf("actors = %q, %q", rows[0].Actor, rows[1].Actor)
	}
	if rows[0].RequestID == nil || *rows[0].RequestID != "req-test-1" || rows[1].RequestID == nil || *rows[1].RequestID != "req-test-2" {
		t.Fatalf("request ids = %v, %v", rows[0].RequestID, rows[1].RequestID)
	}
	if rows[0].Payload["before"] != nil {
		t.Fatalf("the first change replaced nothing, but before = %v", rows[0].Payload["before"])
	}
	after0, _ := rows[0].Payload["after"].(map[string]any)
	if after0["contextual_retrieval"] != "off" || after0["reason"] != "eval baseline organization" {
		t.Fatalf("after = %v", after0)
	}
	if !reflect.DeepEqual(rows[1].Payload["before"], rows[0].Payload["after"]) {
		t.Fatalf("the second change's before is not what the first left: %v vs %v", rows[1].Payload["before"], rows[0].Payload["after"])
	}
	if rows[2].Payload["after"] != nil || rows[2].Payload["before"] == nil {
		t.Fatalf("a reset records what it lifted and leaves nothing: %v", rows[2].Payload)
	}
	if rows[0].Payload["principal_type"] != "service" {
		t.Fatalf("principal type = %v", rows[0].Payload["principal_type"])
	}

	// The audit rows are the organization's own under row-level security.
	err = orgscope.WithOrgScope(ctx, pool, uniqueOrg("audit-other"), func(tx pgx.Tx) error {
		var n int
		if err := tx.QueryRow(ctx, "SELECT count(*) FROM admin_audit_log WHERE org_id = $1", org).Scan(&n); err != nil {
			return err
		}
		if n != 0 {
			return fmt.Errorf("another organization's scope reads %d of this organization's audit rows", n)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

// A change the database refuses leaves neither a half-written policy nor an audit row for a
// change that did not happen, and leaves an earlier policy exactly as it was.
func TestAFailedWriteLeavesNeitherAPolicyNorAnAuditRow(t *testing.T) {
	pool := dbtest.Pool(t)
	store := NewStore(pool)
	org := uniqueOrg("atomic")
	forgetOrgs(t, pool, org)
	ctx := context.Background()

	// Bypasses Validate on purpose: the database's own CHECK (off needs a reason) must hold
	// even when a caller forgets to validate.
	broken := Input{ContextualRetrieval: Off, GraphExtraction: Inherit, BudgetMode: BudgetInherit}
	if _, err := store.Put(ctx, org, broken, operator); err == nil {
		t.Fatal("an off without a reason was stored")
	}
	if n := dbtest.Int(t, pool, "SELECT count(*) FROM org_processing_policy WHERE org_id = $1", org); n != 0 {
		t.Fatalf("a refused write left %d policy row(s)", n)
	}
	if n := len(auditRows(t, pool, org)); n != 0 {
		t.Fatalf("a refused write left %d audit row(s)", n)
	}

	if _, err := store.Put(ctx, org, mustValid(t, offInput()), operator); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Put(ctx, org, broken, operator); err == nil {
		t.Fatal("an off without a reason replaced a good policy")
	}
	got, _ := store.Get(ctx, org)
	if got.ContextualRetrieval != Off || got.Reason == nil || *got.Reason != "eval baseline organization" {
		t.Fatalf("a refused write changed the stored policy: %+v", got)
	}
	if n := len(auditRows(t, pool, org)); n != 1 {
		t.Fatalf("audit rows = %d, want 1", n)
	}
}

// The audit row is part of the change, not a record written next to it: when the audit insert is
// refused, the policy change that was written a statement earlier in the same transaction is not made.
// (A NUL byte is a request id that Postgres cannot store in text. The handler never lets one through,
// it keeps printable ASCII; here it is the fault that makes the audit insert fail after the upsert or
// the delete succeeded.)
func TestAFailedAuditInsertRollsBackThePolicyChange(t *testing.T) {
	pool := dbtest.Pool(t)
	store := NewStore(pool)
	org := uniqueOrg("audit-fail")
	forgetOrgs(t, pool, org)
	ctx := context.Background()
	unauditable := Actor{ID: "corpus-seeder", Type: "service", RequestID: "req\x00nul"}

	// A new policy: neither the row nor an audit row remains.
	if _, err := store.Put(ctx, org, mustValid(t, offInput()), unauditable); err == nil {
		t.Fatal("a change whose audit row cannot be written was accepted")
	}
	if n := dbtest.Int(t, pool, "SELECT count(*) FROM org_processing_policy WHERE org_id = $1", org); n != 0 {
		t.Fatalf("a change without its audit row left %d policy row(s)", n)
	}
	if n := len(auditRows(t, pool, org)); n != 0 {
		t.Fatalf("audit rows = %d, want 0", n)
	}

	// A reset: the policy that was to be lifted stays in force.
	if _, err := store.Put(ctx, org, mustValid(t, offInput()), operator); err != nil {
		t.Fatal(err)
	}
	lift := mustValid(t, Input{ContextualRetrieval: Inherit, GraphExtraction: Inherit, BudgetMode: BudgetInherit})
	if _, err := store.Put(ctx, org, lift, unauditable); err == nil {
		t.Fatal("a reset whose audit row cannot be written was accepted")
	}
	if got, _ := store.Get(ctx, org); !got.Exists || got.ContextualRetrieval != Off {
		t.Fatalf("a reset without its audit row lifted the policy: %+v", got)
	}
	if n := len(auditRows(t, pool, org)); n != 1 {
		t.Fatalf("audit rows = %d, want only the one of the change that was made", n)
	}
}

func TestAPutReplacesTheWholePolicy(t *testing.T) {
	// Exercises the parts of the policy that nothing enforces yet, as they will be stored once graph-index
	// and the budget gate read them: the store itself does not care.
	enforceEverything(t)
	pool := dbtest.Pool(t)
	store := NewStore(pool)
	org := uniqueOrg("replace")
	forgetOrgs(t, pool, org)
	ctx := context.Background()

	pin, reason, figure := "ctx-v2", "cap reached", 12.5
	full := mustValid(t, Input{
		ContextualRetrieval: Off, GraphExtraction: Off, ContextPromptVersion: &pin,
		BudgetMode: BudgetStop, MonthlyBudgetUSD: &figure, Reason: &reason,
	})
	if _, err := store.Put(ctx, org, full, operator); err != nil {
		t.Fatal(err)
	}
	got, _ := store.Get(ctx, org)
	if got.GraphExtraction != Off || got.ContextPromptVersion == nil || *got.ContextPromptVersion != "ctx-v2" ||
		got.BudgetMode != BudgetStop || got.MonthlyBudgetUSD == nil || *got.MonthlyBudgetUSD != 12.5 {
		t.Fatalf("stored = %+v", got)
	}

	if _, err := store.Put(ctx, org, mustValid(t, Input{ContextualRetrieval: On, GraphExtraction: Inherit, BudgetMode: BudgetInherit}), operator); err != nil {
		t.Fatal(err)
	}
	got, _ = store.Get(ctx, org)
	if got.ContextualRetrieval != On || got.GraphExtraction != Inherit || got.ContextPromptVersion != nil ||
		got.BudgetMode != BudgetInherit || got.MonthlyBudgetUSD != nil || got.Reason != nil {
		t.Fatalf("a PUT is a replace, but the earlier policy left traces: %+v", got)
	}
	if n := dbtest.Int(t, pool, "SELECT count(*) FROM org_processing_policy WHERE org_id = $1", org); n != 1 {
		t.Fatalf("rows = %d, want 1", n)
	}
}

// A reset removes the row: a row exists only because an operator set something, so the system
// default (on) is what an absent row means to every consumer.
func TestAResetRemovesTheRowSoTheDefaultAppliesAgain(t *testing.T) {
	pool := dbtest.Pool(t)
	store := NewStore(pool)
	org := uniqueOrg("reset")
	forgetOrgs(t, pool, org)
	ctx := context.Background()

	if _, err := store.Put(ctx, org, mustValid(t, offInput()), operator); err != nil {
		t.Fatal(err)
	}
	if n := dbtest.Int(t, pool, "SELECT count(*) FROM org_processing_policy WHERE org_id = $1", org); n != 1 {
		t.Fatalf("rows after the off = %d", n)
	}
	if _, err := store.Put(ctx, org, mustValid(t, Input{ContextualRetrieval: Inherit, GraphExtraction: Inherit, BudgetMode: BudgetInherit}), operator); err != nil {
		t.Fatal(err)
	}
	if n := dbtest.Int(t, pool, "SELECT count(*) FROM org_processing_policy WHERE org_id = $1", org); n != 0 {
		t.Fatalf("a reset left %d row(s)", n)
	}
	got, _ := store.Get(ctx, org)
	if got.Exists || got.Effective().ContextualRetrieval != "on" {
		t.Fatalf("after the reset: %+v, want the default (on)", got)
	}
}

// Concurrent changes to one organization are serialized, so each audit row's "before" is the state
// that change really replaced: the audit trail is a chain, not a pile.
func TestConcurrentWritesAreSerializedAndEachAuditRowRecordsWhatItReplaced(t *testing.T) {
	// Exercises the parts of the policy that nothing enforces yet, as they will be stored once graph-index
	// and the budget gate read them: the store itself does not care.
	enforceEverything(t)
	pool := dbtest.Pool(t)
	store := NewStore(pool)
	org := uniqueOrg("race")
	forgetOrgs(t, pool, org)
	ctx := context.Background()

	const writers = 8
	var wg sync.WaitGroup
	errs := make(chan error, writers)
	for i := 0; i < writers; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			figure := float64(i + 1)
			in := mustValidNoT(Input{ContextualRetrieval: On, GraphExtraction: Inherit, BudgetMode: BudgetAlertOnly, MonthlyBudgetUSD: &figure})
			_, err := store.Put(ctx, org, in, Actor{ID: fmt.Sprintf("op-%d", i), Type: "service"})
			errs <- err
		}(i)
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatalf("a concurrent Put failed: %v", err)
		}
	}

	rows := auditRows(t, pool, org)
	if len(rows) != writers {
		t.Fatalf("audit rows = %d, want %d", len(rows), writers)
	}
	if rows[0].Payload["before"] != nil {
		t.Fatalf("the first change replaced nothing, but before = %v", rows[0].Payload["before"])
	}
	for i := 1; i < len(rows); i++ {
		if !reflect.DeepEqual(rows[i].Payload["before"], rows[i-1].Payload["after"]) {
			t.Fatalf("audit row %d's before is not the state audit row %d left (the writes were not serialized): %v vs %v",
				i, i-1, rows[i].Payload["before"], rows[i-1].Payload["after"])
		}
	}
	final, _ := store.Get(ctx, org)
	last, _ := rows[len(rows)-1].Payload["after"].(map[string]any)
	if final.MonthlyBudgetUSD == nil || last["monthly_budget_usd"] != *final.MonthlyBudgetUSD {
		t.Fatalf("the stored policy %+v is not the last audited one %v", final, last)
	}
}

func mustValidNoT(in Input) Input {
	out, err := in.Validate()
	if err != nil {
		panic(err)
	}
	return out
}

// A database without the migration answers ErrNotMigrated, which the API turns into a 503 rather
// than a 500: the deploy order is wrong, not the request.
func TestAMissingPolicyTableIsReportedAsNotMigrated(t *testing.T) {
	admin := dbtest.Pool(t)
	ctx := context.Background()
	schema := "nomig_" + strings.ReplaceAll(uuid.NewString(), "-", "")[:10]
	dbtest.Exec(t, admin, "CREATE SCHEMA "+schema)
	dbtest.Exec(t, admin, "GRANT USAGE ON SCHEMA "+schema+" TO dataplane_app")
	t.Cleanup(func() { _, _ = admin.Exec(ctx, "DROP SCHEMA IF EXISTS "+schema+" CASCADE") })

	config, err := pgxpool.ParseConfig(dbtest.URL(t))
	if err != nil {
		t.Fatal(err)
	}
	config.ConnConfig.RuntimeParams["search_path"] = schema
	bare, err := pgxpool.NewWithConfig(ctx, config)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(bare.Close)

	store := NewStore(bare)
	if _, err := store.Get(ctx, "org-nomig"); !errors.Is(err, ErrNotMigrated) {
		t.Fatalf("Get without the table = %v, want ErrNotMigrated", err)
	}
	if _, err := store.Put(ctx, "org-nomig", mustValidNoT(offInput()), operator); !errors.Is(err, ErrNotMigrated) {
		t.Fatalf("Put without the table = %v, want ErrNotMigrated", err)
	}
}

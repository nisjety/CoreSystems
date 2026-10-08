//go:build integration

// Postgres-backed ledger integration test.
//
// Runs only under the `integration` build tag against a real Postgres pointed
// to by COST_CORE_TEST_DATABASE_URL, e.g. the Model Plane compose database:
//
//	COST_CORE_TEST_DATABASE_URL=postgresql://postgres:postgres@localhost:5432/session_core \
//	  go test -tags integration ./services/cost-core/internal/postgres/...
//
// The test applies the migration, exercises record/usage/run/aggregate/budget,
// and cleans up its own rows. It skips (not fails) when the env var is unset so
// the default `go test ./...` stays hermetic.
package postgres

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

func TestPostgresLedger_Integration(t *testing.T) {
	dsn := os.Getenv("COST_CORE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("COST_CORE_TEST_DATABASE_URL not set; skipping Postgres integration test")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	pool, err := Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	// t.Cleanup, not defer: cleanups run after the test body returns, so a
	// deferred Close would shut the pool before the row cleanup below runs
	// and leave rows behind that break the next run's global run rollup.
	t.Cleanup(pool.Close)

	applyMigration(ctx, t, pool)

	org := "itest-org-" + time.Now().Format("150405.000")
	otherOrg := org + "-other"
	t.Cleanup(func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM cost_entries WHERE org_id IN ($1, $2)`, org, otherOrg)
	})

	store, err := New(pool)
	if err != nil {
		t.Fatalf("new store: %v", err)
	}

	must := func(e ledger.Entry) {
		t.Helper()
		if err := store.RecordEntry(ctx, e); err != nil {
			t.Fatalf("record: %v", err)
		}
	}
	must(ledger.Entry{OrgID: org, UserID: "u1", RunID: "run1", Model: "gpt", InputTokens: 100, OutputTokens: 40, CostUSD: 0.25, IdempotencyKey: "itest-k1"})
	must(ledger.Entry{OrgID: org, UserID: "u1", RunID: "run1", Model: "gpt", InputTokens: 100, OutputTokens: 40, CostUSD: 0.25, IdempotencyKey: "itest-k1"}) // dup
	// Cache-token telemetry rides alongside the ordinary token counts (native-
	// compaction migration prerequisite): this entry's legs must survive the
	// round trip through every read path exercised below.
	must(ledger.Entry{OrgID: org, UserID: "u2", RunID: "run1", Model: "claude", InputTokens: 10, CostUSD: 0.5, CacheReadInputTokens: 6, CacheCreationInputTokens: 3})
	must(ledger.Entry{OrgID: otherOrg, UserID: "u3", ProducerID: "service:model-gateway", InputTokens: 9, IdempotencyKey: "itest-k1"})

	// Idempotent: the duplicate key must collapse to one row.
	usage, err := store.GetUsage(ctx, org, "u1")
	if err != nil {
		t.Fatalf("get usage: %v", err)
	}
	if usage.EntryCount != 1 || usage.TotalInputTokens != 100 {
		t.Fatalf("idempotency failed: %+v", usage)
	}
	otherUsage, err := store.GetUsage(ctx, otherOrg, "u3")
	if err != nil || otherUsage.EntryCount != 1 || otherUsage.TotalInputTokens != 9 {
		t.Fatalf("tenant-scoped idempotency failed: usage=%+v err=%v", otherUsage, err)
	}

	// Run rollup spans both users.
	run, err := store.GetRunUsage(ctx, "run1")
	if err != nil {
		t.Fatalf("get run usage: %v", err)
	}
	if run.EntryCount != 2 || run.TotalInputTokens != 110 {
		t.Fatalf("run rollup wrong: %+v", run)
	}
	if run.TotalCacheReadInputTokens != 6 || run.TotalCacheCreationInputTokens != 3 {
		t.Fatalf("run cache-token rollup wrong: %+v", run)
	}

	// Org aggregate.
	agg, err := store.Aggregate(ctx, ledger.AggregateFilter{OrgID: org})
	if err != nil {
		t.Fatalf("aggregate: %v", err)
	}
	if agg.EntryCount != 2 || agg.TotalCostUSD < 0.74 || agg.TotalCostUSD > 0.76 {
		t.Fatalf("aggregate wrong: %+v", agg)
	}
	if agg.TotalCacheReadInputTokens != 6 || agg.TotalCacheCreationInputTokens != 3 {
		t.Fatalf("aggregate cache-token totals wrong: %+v", agg)
	}

	// Budget cap.
	if err := store.CheckBudget(ctx, org, "u1", 0.10, 0); err == nil {
		t.Fatalf("expected cost cap to be exceeded")
	}
	if err := store.CheckBudget(ctx, org, "u1", 100, 0); err != nil {
		t.Fatalf("unexpected budget rejection: %v", err)
	}

	// List newest-first.
	entries, err := store.ListEntries(ctx, ledger.AggregateFilter{OrgID: org}, 10)
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	if len(entries) != 2 {
		t.Fatalf("list count = %d, want 2", len(entries))
	}
	var sawCacheEntry bool
	for _, e := range entries {
		if e.UserID == "u2" {
			sawCacheEntry = true
			if e.CacheReadInputTokens != 6 || e.CacheCreationInputTokens != 3 {
				t.Fatalf("listed entry cache tokens wrong: %+v", e)
			}
		}
	}
	if !sawCacheEntry {
		t.Fatalf("expected the u2 cache-bearing entry in the listing: %+v", entries)
	}
}

// TestPostgresLedger_PurgeOrgIsolatesOtherOrg is the mandatory GDPR safety
// test for the org-erasure consumer: purging one org's cost_entries rows must
// never touch another org's rows, and must be safe to run twice (NATS
// redelivery) on the same org.
func TestPostgresLedger_PurgeOrgIsolatesOtherOrg(t *testing.T) {
	dsn := os.Getenv("COST_CORE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("COST_CORE_TEST_DATABASE_URL not set; skipping Postgres integration test")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	pool, err := Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	// t.Cleanup, not defer: cleanups run after the test body returns, so a
	// deferred Close would shut the pool before the row cleanup below runs
	// and leave rows behind that break the next run's global run rollup.
	t.Cleanup(pool.Close)

	applyMigration(ctx, t, pool)

	orgA := "itest-purge-a-" + time.Now().Format("150405.000")
	orgB := orgA + "-b"
	t.Cleanup(func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM cost_entries WHERE org_id IN ($1, $2)`, orgA, orgB)
	})

	store, err := New(pool)
	if err != nil {
		t.Fatalf("new store: %v", err)
	}

	must := func(e ledger.Entry) {
		t.Helper()
		if err := store.RecordEntry(ctx, e); err != nil {
			t.Fatalf("record: %v", err)
		}
	}
	must(ledger.Entry{OrgID: orgA, UserID: "u1", InputTokens: 100, CostUSD: 1, IdempotencyKey: "purge-a-1"})
	must(ledger.Entry{OrgID: orgB, UserID: "u2", InputTokens: 200, CostUSD: 2, IdempotencyKey: "purge-b-1"})

	if err := store.PurgeOrg(ctx, orgA); err != nil {
		t.Fatalf("PurgeOrg(orgA): %v", err)
	}

	if _, err := store.GetUsage(ctx, orgA, "u1"); !errors.Is(err, ledger.ErrUsageNotFound) {
		t.Fatalf("orgA usage = %v, want ErrUsageNotFound after purge", err)
	}
	entriesA, err := store.ListEntries(ctx, ledger.AggregateFilter{OrgID: orgA}, 10)
	if err != nil || len(entriesA) != 0 {
		t.Fatalf("orgA entries = %v (err=%v), want none after purge", entriesA, err)
	}

	usageB, err := store.GetUsage(ctx, orgB, "u2")
	if err != nil {
		t.Fatalf("orgB usage should survive purge of orgA: %v", err)
	}
	if usageB.TotalInputTokens != 200 || usageB.EntryCount != 1 {
		t.Fatalf("orgB usage corrupted by orgA purge: %+v", usageB)
	}
	// Durable delivery can replay a usage event after erasure. Neither an old
	// idempotency key nor a new one may recreate this organization's ledger.
	for _, key := range []string{"purge-a-1", "purge-a-replay-new"} {
		if err := store.RecordEntry(ctx, ledger.Entry{OrgID: orgA, UserID: "u1", InputTokens: 100, IdempotencyKey: key}); !errors.Is(err, ledger.ErrOrgErased) {
			t.Fatalf("erased org replay %q: %v, want ErrOrgErased", key, err)
		}
	}
	if err := store.RecordEntry(ctx, ledger.Entry{OrgID: orgB, UserID: "u2", InputTokens: 10, IdempotencyKey: "purge-b-after"}); err != nil {
		t.Fatalf("other org must still accept usage after erasure: %v", err)
	}

	// Idempotent: a redelivered purge event must not error and must not touch orgB.
	if err := store.PurgeOrg(ctx, orgA); err != nil {
		t.Fatalf("second PurgeOrg(orgA) should be a no-op, got: %v", err)
	}
	if _, err := store.GetUsage(ctx, orgB, "u2"); err != nil {
		t.Fatalf("orgB usage should still be intact after redelivered purge: %v", err)
	}
}

// applyMigration runs the up migration so the test is self-contained on a fresh
// database.
func TestReportingPeriodIsScopedAndDoesNotEraseLedger(t *testing.T) {
	dsn := os.Getenv("COST_CORE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("COST_CORE_TEST_DATABASE_URL not set")
	}
	ctx := context.Background()
	pool, err := Connect(ctx, dsn)
	if err != nil {
		t.Fatal(err)
	}
	defer pool.Close()
	applyMigration(ctx, t, pool)
	store, _ := New(pool)
	org := "reporting-test-" + time.Now().Format("150405.000000")
	defer pool.Exec(ctx, `DELETE FROM usage_reporting_periods WHERE org_id=$1`, org)
	defer pool.Exec(ctx, `DELETE FROM cost_entries WHERE org_id=$1`, org)
	if err := store.RecordEntry(ctx, ledger.Entry{OrgID: org, UserID: "actor", InputTokens: 10, CostBasis: "subscription"}); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 2; i++ {
		if _, err := pool.Exec(ctx, `INSERT INTO usage_reporting_periods(org_id,requested_by,reason) VALUES($1,'test-operator','test')`, org); err != nil {
			t.Fatal(err)
		}
	}
	start, err := store.ReportingStart(ctx, org)
	if err != nil || start == nil {
		t.Fatalf("missing reporting period: %v", err)
	}
	other, err := store.ReportingStart(ctx, org+"-other")
	if err != nil || other != nil {
		t.Fatal("reporting period leaked to another organization")
	}
	before, _ := store.Aggregate(ctx, ledger.AggregateFilter{OrgID: org})
	after, _ := store.Aggregate(ctx, ledger.AggregateFilter{OrgID: org, Since: *start})
	if before.EntryCount != 1 || after.EntryCount != 0 {
		t.Fatal("reset either lost history or retained earlier usage in the new period")
	}
	if err := store.PurgeOrg(ctx, org); err != nil {
		t.Fatal(err)
	}
	erased, err := store.ReportingStart(ctx, org)
	if err != nil || erased != nil {
		t.Fatal("erasure retained organization reporting metadata")
	}
}

func applyMigration(ctx context.Context, t *testing.T, pool *pgxpool.Pool) {
	t.Helper()
	paths, err := filepath.Glob(filepath.Join("..", "..", "migrations", "*.up.sql"))
	if err != nil {
		t.Fatalf("list migrations: %v", err)
	}
	for _, path := range paths {
		sqlBytes, readErr := os.ReadFile(path)
		if readErr != nil {
			t.Fatalf("read migration %s: %v", path, readErr)
		}
		if _, execErr := pool.Exec(ctx, string(sqlBytes)); execErr != nil {
			t.Fatalf("apply migration %s: %v", path, execErr)
		}
	}
}

// TestPostgresLedger_CostBasisRoundTrip pins migration 0005: a reported cost
// basis survives the round trip, an unreported one is stored as NULL rather
// than an empty string, and the migrations stay safe to re-run on every boot.
func TestPostgresLedger_CostBasisRoundTrip(t *testing.T) {
	dsn := os.Getenv("COST_CORE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("COST_CORE_TEST_DATABASE_URL not set; skipping Postgres integration test")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	pool, err := Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	// t.Cleanup, not defer: cleanups run after the test body returns, so a
	// deferred Close would shut the pool before the row cleanup below runs
	// and leave rows behind that break the next run's global run rollup.
	t.Cleanup(pool.Close)

	applyMigration(ctx, t, pool)
	// The entrypoint re-applies every migration on each boot.
	applyMigration(ctx, t, pool)

	org := "itest-basis-" + time.Now().Format("150405.000")
	t.Cleanup(func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM cost_entries WHERE org_id = $1`, org)
	})

	store, err := New(pool)
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	for requestID, basis := range map[string]string{
		"req-sub":     ledger.CostBasisSubscription,
		"req-metered": ledger.CostBasisMetered,
		"req-old":     ledger.CostBasisUnknown,
	} {
		if err := store.RecordEntry(ctx, ledger.Entry{
			OrgID: org, UserID: "u1", RequestID: requestID, Model: "gpt-5.6-terra",
			InputTokens: 1_800, OutputTokens: 120, CostBasis: basis, IdempotencyKey: requestID,
		}); err != nil {
			t.Fatalf("record %s: %v", requestID, err)
		}
	}

	entries, err := store.ListEntries(ctx, ledger.AggregateFilter{OrgID: org}, 10)
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	got := map[string]string{}
	for _, e := range entries {
		got[e.RequestID] = e.CostBasis
	}
	if len(got) != 3 || got["req-sub"] != ledger.CostBasisSubscription ||
		got["req-metered"] != ledger.CostBasisMetered || got["req-old"] != ledger.CostBasisUnknown {
		t.Fatalf("listed cost bases = %v", got)
	}

	var nullRows int
	if err := pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM cost_entries WHERE org_id = $1 AND cost_basis IS NULL`, org,
	).Scan(&nullRows); err != nil {
		t.Fatalf("count NULL bases: %v", err)
	}
	if nullRows != 1 {
		t.Fatalf("rows with a NULL cost_basis = %d, want 1 (the unreported one)", nullRows)
	}
	users, err := store.AggregateUsers(ctx, ledger.AggregateFilter{OrgID: org})
	if err != nil || len(users) != 1 || users[0].UserID != "u1" || users[0].EntryCount != 3 ||
		users[0].MeteredEntries != 1 || users[0].SubscriptionEntries != 1 || users[0].UnknownBasisEntries != 1 ||
		users[0].InputTokens != 5400 || users[0].OutputTokens != 360 {
		t.Fatalf("user report must reconcile all billing bases and recorded tokens: users=%+v err=%v", users, err)
	}

	// A token budget caps metered tokens only: the subscription entry stays
	buckets, err := store.UsageAnalytics(ctx, ledger.AggregateFilter{OrgID: org, UserID: "u1"})
	if err != nil || len(buckets) != 3 {
		t.Fatalf("analytics: %+v %v", buckets, err)
	}
	for _, b := range buckets {
		if b.Requests != 1 || b.InputTokens != 1800 || b.OutputTokens != 120 {
			t.Fatalf("bucket did not reconcile: %+v", b)
		}
		if b.Basis != "metered" && b.MeteredCostUSD != 0 {
			t.Fatal("non-metered usage became API cost")
		}
	}
	other, err := store.UsageAnalytics(ctx, ledger.AggregateFilter{OrgID: org + "-other"})
	if err != nil || len(other) != 0 {
		t.Fatal("analytics leaked across tenants")
	}
	other, err = store.UsageAnalytics(ctx, ledger.AggregateFilter{OrgID: org, UserID: "u2"})
	if err != nil || len(other) != 0 {
		t.Fatal("analytics leaked across users")
	}
	other, err = store.UsageAnalytics(ctx, ledger.AggregateFilter{OrgID: org, Since: time.Now().Add(time.Hour)})
	if err != nil || len(other) != 0 {
		t.Fatal("analytics ignored reporting start")
	}

	// A token budget caps metered tokens only: the subscription entry stays
	// in the totals but never counts against max_tokens.
	usage, err := store.GetUsage(ctx, org, "u1")
	if err != nil {
		t.Fatalf("get usage: %v", err)
	}
	agg, err := store.Aggregate(ctx, ledger.AggregateFilter{OrgID: org})
	if err != nil {
		t.Fatalf("aggregate: %v", err)
	}
	for name, u := range map[string]*ledger.Usage{"usage": usage, "aggregate": agg} {
		if u.TotalInputTokens != 3*1_800 || u.MeteredInputTokens != 2*1_800 || u.MeteredOutputTokens != 2*120 {
			t.Fatalf("%s tokens = %+v, want the subscription entry in the totals only", name, u)
		}
	}
	if err := store.CheckBudget(ctx, org, "u1", 0, 2*(1_800+120)+1); err != nil {
		t.Fatalf("subscription tokens counted against max_tokens: %v", err)
	}
}

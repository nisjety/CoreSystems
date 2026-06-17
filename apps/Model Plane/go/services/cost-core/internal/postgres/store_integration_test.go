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
	defer pool.Close()

	applyMigration(ctx, t, pool)

	org := "itest-org-" + time.Now().Format("150405.000")
	t.Cleanup(func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM cost_entries WHERE org_id = $1`, org)
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
	must(ledger.Entry{OrgID: org, UserID: "u2", RunID: "run1", Model: "claude", InputTokens: 10, CostUSD: 0.5})

	// Idempotent: the duplicate key must collapse to one row.
	usage, err := store.GetUsage(ctx, org, "u1")
	if err != nil {
		t.Fatalf("get usage: %v", err)
	}
	if usage.EntryCount != 1 || usage.TotalInputTokens != 100 {
		t.Fatalf("idempotency failed: %+v", usage)
	}

	// Run rollup spans both users.
	run, err := store.GetRunUsage(ctx, "run1")
	if err != nil {
		t.Fatalf("get run usage: %v", err)
	}
	if run.EntryCount != 2 || run.TotalInputTokens != 110 {
		t.Fatalf("run rollup wrong: %+v", run)
	}

	// Org aggregate.
	agg, err := store.Aggregate(ctx, ledger.AggregateFilter{OrgID: org})
	if err != nil {
		t.Fatalf("aggregate: %v", err)
	}
	if agg.EntryCount != 2 || agg.TotalCostUSD < 0.74 || agg.TotalCostUSD > 0.76 {
		t.Fatalf("aggregate wrong: %+v", agg)
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
}

// applyMigration runs the up migration so the test is self-contained on a fresh
// database.
func applyMigration(ctx context.Context, t *testing.T, pool *pgxpool.Pool) {
	t.Helper()
	path := filepath.Join("..", "..", "migrations", "0001_cost_ledger.up.sql")
	sqlBytes, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	if _, err := pool.Exec(ctx, string(sqlBytes)); err != nil {
		t.Fatalf("apply migration: %v", err)
	}
}

//go:build integration

package registry

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
)

// setupRegistryDB spins a throwaway Postgres, applies migration 0003 (which
// creates capabilities + capability_scopes), and returns a connected pool.
func setupRegistryDB(t *testing.T) *pgxpool.Pool {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("capabilities"),
		tcpostgres.WithUsername("test"),
		tcpostgres.WithPassword("test"),
		testcontainers.WithWaitStrategy(
			wait.ForLog("database system is ready to accept connections").
				WithOccurrence(2).WithStartupTimeout(60*time.Second),
		),
	)
	if err != nil {
		t.Fatalf("start postgres: %v", err)
	}
	t.Cleanup(func() {
		c, cc := context.WithTimeout(context.Background(), 30*time.Second)
		defer cc()
		_ = container.Terminate(c)
	})

	dsn, err := container.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("conn string: %v", err)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	t.Cleanup(pool.Close)

	sqlBytes, err := os.ReadFile(filepath.Join("..", "..", "migrations", "0003_capabilities_registry.up.sql"))
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	if _, err := pool.Exec(ctx, string(sqlBytes)); err != nil {
		t.Fatalf("apply migration: %v", err)
	}
	return pool
}

// seedCap inserts a minimal capability row (FK target for capability_scopes).
func seedCap(t *testing.T, store *CapabilitiesStore, id, name, kind string) {
	t.Helper()
	if err := store.Upsert(context.Background(), &CapabilityRow{
		ID: id, OrgID: "global", Kind: kind, Name: name, Version: "1.0.0",
		RiskLevel: "low", Scope: "org", Enabled: true,
	}); err != nil {
		t.Fatalf("seed cap %s: %v", id, err)
	}
}

func TestScopeStore_GrantResolveRevoke(t *testing.T) {
	pool := setupRegistryDB(t)
	ctx := context.Background()
	capStore, _ := NewCapabilitiesStore(pool)
	scopes, _ := NewScopeStore(pool)

	seedCap(t, capStore, "cap.x", "X", "tool")
	seedCap(t, capStore, "cap.y", "Y", "tool")

	// Grant cap.x to org "acme", cap.y to all orgs (wildcard).
	if _, err := scopes.Grant(ctx, "", "cap.x", ScopeKindOrg, "acme", "tester"); err != nil {
		t.Fatalf("grant cap.x: %v", err)
	}
	if _, err := scopes.Grant(ctx, "", "cap.y", ScopeKindOrg, "*", "tester"); err != nil {
		t.Fatalf("grant cap.y: %v", err)
	}

	// Resolve for org "acme" → both cap.x (exact) and cap.y (wildcard).
	ids, err := scopes.ResolveForScope(ctx, ScopeKindOrg, "acme")
	if err != nil {
		t.Fatalf("resolve acme: %v", err)
	}
	if !contains(ids, "cap.x") || !contains(ids, "cap.y") {
		t.Fatalf("expected [cap.x cap.y] for acme, got %v", ids)
	}

	// Resolve for org "other" → only cap.y (wildcard); cap.x not granted.
	ids, err = scopes.ResolveForScope(ctx, ScopeKindOrg, "other")
	if err != nil {
		t.Fatalf("resolve other: %v", err)
	}
	if contains(ids, "cap.x") || !contains(ids, "cap.y") {
		t.Fatalf("expected only [cap.y] for other, got %v", ids)
	}

	// IsGrantedForScope honours wildcard + exact.
	if ok, _ := scopes.IsGrantedForScope(ctx, "cap.x", ScopeKindOrg, "acme"); !ok {
		t.Fatalf("cap.x should be granted for acme")
	}
	if ok, _ := scopes.IsGrantedForScope(ctx, "cap.x", ScopeKindOrg, "other"); ok {
		t.Fatalf("cap.x should NOT be granted for other")
	}
	if ok, _ := scopes.IsGrantedForScope(ctx, "cap.y", ScopeKindOrg, "anything"); !ok {
		t.Fatalf("cap.y wildcard should cover any org")
	}

	// HasAnyGrants distinguishes governed vs ungoverned.
	if ok, _ := scopes.HasAnyGrants(ctx, "cap.x", ScopeKindOrg); !ok {
		t.Fatalf("cap.x should report org grants")
	}
	if ok, _ := scopes.HasAnyGrants(ctx, "cap.x", ScopeKindAgent); ok {
		t.Fatalf("cap.x should report NO agent grants")
	}

	// Revoke cap.x for acme → resolution drops it.
	n, err := scopes.Revoke(ctx, "cap.x", ScopeKindOrg, "acme")
	if err != nil {
		t.Fatalf("revoke: %v", err)
	}
	if n != 1 {
		t.Fatalf("expected 1 grant revoked, got %d", n)
	}
	ids, _ = scopes.ResolveForScope(ctx, ScopeKindOrg, "acme")
	if contains(ids, "cap.x") {
		t.Fatalf("cap.x should be revoked for acme, got %v", ids)
	}
}

func TestCapabilitiesStore_RankedList_OrdersByScore(t *testing.T) {
	pool := setupRegistryDB(t)
	ctx := context.Background()
	store, _ := NewCapabilitiesStore(pool)

	// Insert two capabilities; bump one's health columns so it must rank first.
	seedCap(t, store, "cap.good", "Good", "tool")
	seedCap(t, store, "cap.bad", "Bad", "tool")
	if _, err := pool.Exec(ctx, `
		UPDATE capabilities SET success_rate=0.99, approval_rate=0.99, operator_rating=5,
		       p95_latency_ms=60, mean_cost_usd=0.001, incident_count=0, schema_fail_rate=0.0,
		       rollout_state='stable'
		WHERE id='cap.good'`); err != nil {
		t.Fatalf("bump good: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		UPDATE capabilities SET success_rate=0.40, approval_rate=0.40, operator_rating=1,
		       p95_latency_ms=4000, mean_cost_usd=0.9, incident_count=8, schema_fail_rate=0.5,
		       rollout_state='canary'
		WHERE id='cap.bad'`); err != nil {
		t.Fatalf("bump bad: %v", err)
	}

	ranked, err := store.RankedList(ctx, "global", "tool", nil, 50)
	if err != nil {
		t.Fatalf("ranked list: %v", err)
	}
	if len(ranked) < 2 {
		t.Fatalf("expected >=2 ranked rows, got %d", len(ranked))
	}
	if ranked[0].Row.ID != "cap.good" {
		t.Fatalf("expected cap.good ranked first, got %s (score %v)", ranked[0].Row.ID, ranked[0].Score)
	}
	if ranked[0].Score <= ranked[1].Score {
		t.Fatalf("expected descending scores, got %v then %v", ranked[0].Score, ranked[1].Score)
	}

	// id-restricted ranking returns only the requested set.
	only, err := store.RankedList(ctx, "global", "", []string{"cap.bad"}, 50)
	if err != nil {
		t.Fatalf("ranked subset: %v", err)
	}
	if len(only) != 1 || only[0].Row.ID != "cap.bad" {
		t.Fatalf("expected only cap.bad, got %v", only)
	}
}

func contains(xs []string, want string) bool {
	for _, x := range xs {
		if x == want {
			return true
		}
	}
	return false
}

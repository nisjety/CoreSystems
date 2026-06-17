//go:build integration

// These tests exercise the real Postgres-backed store. They require a reachable
// Postgres and are skipped unless LETTA_BRIDGE_TEST_DATABASE_URL is set, e.g.:
//
//	LETTA_BRIDGE_TEST_DATABASE_URL=postgres://user:pass@localhost:5432/db?sslmode=disable \
//	  go test -tags integration ./services/letta-bridge/internal/pgstore/...
//
// The store creates its own table (letta_memory_blocks) idempotently; each test
// scopes its data with a unique org id and cleans up after itself, so it is safe
// to point at a shared dev database.
package pgstore

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// uniqueOrg returns a per-test org id so concurrent runs against a shared dev
// database never collide. Avoids pulling a UUID dependency into the test.
func uniqueOrg() string {
	return fmt.Sprintf("test-org-%d", time.Now().UnixNano())
}

func newTestStore(t *testing.T) (*Store, *pgxpool.Pool, string) {
	t.Helper()
	dsn := os.Getenv("LETTA_BRIDGE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("LETTA_BRIDGE_TEST_DATABASE_URL not set; skipping pgstore integration test")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("open pool: %v", err)
	}
	t.Cleanup(pool.Close)

	store, err := New(ctx, pool)
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	org := uniqueOrg()
	t.Cleanup(func() {
		cctx, ccancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer ccancel()
		_, _ = pool.Exec(cctx, "DELETE FROM letta_memory_blocks WHERE org_id = $1", org)
	})
	return store, pool, org
}

func TestPgStorePutValidation(t *testing.T) {
	store, _, org := newTestStore(t)
	ctx := context.Background()
	if _, err := store.Put(ctx, org, "", "MEMORY", "m1", "x"); err == nil {
		t.Fatal("expected error for empty threadID")
	}
	if _, err := store.Put(ctx, "", "t1", "MEMORY", "m1", "x"); err == nil {
		t.Fatal("expected error for empty orgID")
	}
}

func TestPgStorePutSearchRoundTrip(t *testing.T) {
	store, _, org := newTestStore(t)
	ctx := context.Background()

	if _, err := store.Put(ctx, org, "thread-1", "MEMORY", "m1", "user prefers dark mode"); err != nil {
		t.Fatalf("put m1: %v", err)
	}
	if _, err := store.Put(ctx, org, "thread-1", "MEMORY", "m2", "deadline is friday"); err != nil {
		t.Fatalf("put m2: %v", err)
	}

	hits, err := store.Search(ctx, org, "", "dark", nil, time.Time{}, 10)
	if err != nil {
		t.Fatalf("search: %v", err)
	}
	if len(hits) != 1 || hits[0].MemoryID != "m1" {
		t.Fatalf("expected 1 hit m1, got %+v", hits)
	}
	if hits[0].Score != 0.5 {
		t.Errorf("substring (non-prefix) match should score 0.5, got %v", hits[0].Score)
	}

	// Prefix match scores 1.0.
	prefixHits, err := store.Search(ctx, org, "", "user", nil, time.Time{}, 10)
	if err != nil {
		t.Fatalf("search prefix: %v", err)
	}
	if len(prefixHits) != 1 || prefixHits[0].Score != 1.0 {
		t.Fatalf("expected prefix score 1.0, got %+v", prefixHits)
	}
}

func TestPgStoreUpsertOverwrites(t *testing.T) {
	store, _, org := newTestStore(t)
	ctx := context.Background()

	if _, err := store.Put(ctx, org, "t1", "MEMORY", "dup", "v1"); err != nil {
		t.Fatalf("put v1: %v", err)
	}
	if _, err := store.Put(ctx, org, "t1", "MEMORY", "dup", "v2"); err != nil {
		t.Fatalf("put v2: %v", err)
	}
	hits, err := store.Search(ctx, org, "", "", nil, time.Time{}, 10)
	if err != nil {
		t.Fatalf("search: %v", err)
	}
	if len(hits) != 1 {
		t.Fatalf("upsert should yield 1 row, got %d", len(hits))
	}
	if hits[0].Content != "v2" {
		t.Errorf("expected content v2 after upsert, got %q", hits[0].Content)
	}
}

func TestPgStoreScopingAndFilters(t *testing.T) {
	store, _, org := newTestStore(t)
	ctx := context.Background()

	mustPut := func(thread, topic, id, content string) {
		if _, err := store.Put(ctx, org, thread, topic, id, content); err != nil {
			t.Fatalf("put %s: %v", id, err)
		}
	}
	mustPut("t1", "MEMORY", "a", "alpha")
	mustPut("t2", "MEMORY", "b", "beta")
	mustPut("t1", "NOTE", "c", "gamma")

	// thread filter
	hits, err := store.Search(ctx, org, "t1", "", nil, time.Time{}, 10)
	if err != nil {
		t.Fatalf("thread search: %v", err)
	}
	if len(hits) != 2 {
		t.Fatalf("thread t1 should yield 2, got %d", len(hits))
	}

	// topic filter
	topicHits, err := store.Search(ctx, org, "", "", []string{"NOTE"}, time.Time{}, 10)
	if err != nil {
		t.Fatalf("topic search: %v", err)
	}
	if len(topicHits) != 1 || topicHits[0].MemoryID != "c" {
		t.Fatalf("topic NOTE should yield only c, got %+v", topicHits)
	}

	// org isolation: a different org sees nothing.
	otherOrg := "other-" + org
	otherHits, err := store.Search(ctx, otherOrg, "", "", nil, time.Time{}, 10)
	if err != nil {
		t.Fatalf("other org search: %v", err)
	}
	if len(otherHits) != 0 {
		t.Fatalf("org isolation broken: other org saw %d rows", len(otherHits))
	}

	// limit
	limited, err := store.Search(ctx, org, "", "", nil, time.Time{}, 1)
	if err != nil {
		t.Fatalf("limited search: %v", err)
	}
	if len(limited) != 1 {
		t.Fatalf("limit 1 should yield 1, got %d", len(limited))
	}
}

func TestPgStoreLiteralWildcards(t *testing.T) {
	store, _, org := newTestStore(t)
	ctx := context.Background()

	if _, err := store.Put(ctx, org, "t1", "MEMORY", "pct", "battery at 50% now"); err != nil {
		t.Fatalf("put: %v", err)
	}
	if _, err := store.Put(ctx, org, "t1", "MEMORY", "plain", "battery low"); err != nil {
		t.Fatalf("put: %v", err)
	}
	// "%" must be matched literally, not as an ILIKE wildcard.
	hits, err := store.Search(ctx, org, "", "50%", nil, time.Time{}, 10)
	if err != nil {
		t.Fatalf("search: %v", err)
	}
	if len(hits) != 1 || hits[0].MemoryID != "pct" {
		t.Fatalf("expected literal %% match only on pct, got %+v", hits)
	}
}

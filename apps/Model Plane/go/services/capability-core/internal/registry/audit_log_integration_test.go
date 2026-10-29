//go:build integration

package registry

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
)

// setupCapabilitiesStore spins a throwaway Postgres, applies the capabilities
// registry migration (which creates registry_audit_log), and returns a store.
func setupCapabilitiesStore(t *testing.T) *CapabilitiesStore {
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

	store, err := NewCapabilitiesStore(pool)
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	return store
}

func TestQueryAuditLog_RoundTrip(t *testing.T) {
	store := setupCapabilitiesStore(t)
	ctx := context.Background()

	// Append three entries, spaced so the ts ordering is deterministic.
	if err := store.AppendAuditLog(ctx, "capability", "cap-1", "created", "alice", "org-1", []byte(`{"field":"x"}`)); err != nil {
		t.Fatalf("append 1: %v", err)
	}
	time.Sleep(3 * time.Millisecond)
	if err := store.AppendAuditLog(ctx, "skill", "sk-1", "updated", "bob", "org-1", nil); err != nil {
		t.Fatalf("append 2: %v", err)
	}
	time.Sleep(3 * time.Millisecond)
	if err := store.AppendAuditLog(ctx, "capability", "cap-1", "quarantined", "carol", "org-1", nil); err != nil {
		t.Fatalf("append 3: %v", err)
	}

	// All entries, newest-first.
	all, err := store.QueryAuditLog(ctx, "", "", 0)
	if err != nil {
		t.Fatalf("query all: %v", err)
	}
	if len(all) != 3 {
		t.Fatalf("want 3 entries, got %d", len(all))
	}
	if all[0].Action != "quarantined" {
		t.Fatalf("expected newest-first (quarantined), got %q", all[0].Action)
	}

	// Filter by entity_id.
	c1, err := store.QueryAuditLog(ctx, "", "cap-1", 0)
	if err != nil {
		t.Fatalf("query by entity_id: %v", err)
	}
	if len(c1) != 2 {
		t.Fatalf("entity_id=cap-1 should match 2, got %d", len(c1))
	}

	// Filter by entity_kind + limit.
	caps, err := store.QueryAuditLog(ctx, "capability", "", 1)
	if err != nil {
		t.Fatalf("query by kind+limit: %v", err)
	}
	if len(caps) != 1 || caps[0].EntityKind != "capability" {
		t.Fatalf("kind=capability limit=1 wrong: %+v", caps)
	}

	// diff_json round-trips as embedded JSON (not base64-encoded bytes).
	var createdDiff string
	for _, e := range all {
		if e.Action == "created" {
			createdDiff = string(e.Diff)
		}
	}
	if !strings.Contains(createdDiff, `"field"`) || !strings.Contains(createdDiff, `"x"`) {
		t.Fatalf("diff should embed JSON, got %q", createdDiff)
	}
}

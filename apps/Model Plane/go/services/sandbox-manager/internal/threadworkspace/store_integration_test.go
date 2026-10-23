//go:build integration

package threadworkspace

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
)

// setupThreadDB spins a throwaway Postgres and applies migration 0005, the
// thread tables this store reads and writes.
func setupThreadDB(t *testing.T) *pgxpool.Pool {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("sandbox_manager"),
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

	sqlBytes, err := os.ReadFile(filepath.Join("..", "..", "migrations", "0005_thread_workspace.up.sql"))
	if err != nil {
		t.Fatalf("read migration 0005: %v", err)
	}
	if _, err := pool.Exec(ctx, string(sqlBytes)); err != nil {
		t.Fatalf("apply migration 0005: %v", err)
	}
	return pool
}

// TestThreadStore_ApplyIsCompareAndSwapAndCapped runs the real SQL: the header
// row lock, the per-path upsert and delete, and the revision and counters.
func TestThreadStore_ApplyIsCompareAndSwapAndCapped(t *testing.T) {
	pool := setupThreadDB(t)
	ctx := context.Background()
	store, err := NewStore(pool)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	limits := Limits{MaxFiles: 2, MaxBytes: 1 << 20}

	first, err := store.Apply(ctx, "org-a", "user-a", "thread-1", []Change{
		{Path: "a.txt", ContentHash: "sha256:one", SizeBytes: 3},
	}, nil, limits)
	if err != nil {
		t.Fatalf("first apply: %v", err)
	}
	if first.Revision != 1 || len(first.Conflicts) != 0 || first.FileCount != 1 {
		t.Fatalf("first apply = %+v, want revision 1, one file, no conflict", first)
	}

	stale, err := store.Apply(ctx, "org-a", "user-a", "thread-1", []Change{
		{Path: "a.txt", ContentHash: "sha256:two", SizeBytes: 3, BaseHash: "sha256:stale"},
	}, nil, limits)
	if err != nil {
		t.Fatalf("stale apply: %v", err)
	}
	if len(stale.Conflicts) != 1 || stale.Revision != 1 {
		t.Fatalf("stale apply = %+v, want the conflict reported and the revision unchanged", stale)
	}

	if _, err := store.Apply(ctx, "org-a", "user-a", "thread-1", []Change{
		{Path: "b.txt", ContentHash: "sha256:b", SizeBytes: 1},
		{Path: "c.txt", ContentHash: "sha256:c", SizeBytes: 1},
	}, nil, limits); !errors.Is(err, ErrQuotaExceeded) {
		t.Fatalf("third file under a two-file cap: err = %v, want ErrQuotaExceeded", err)
	}

	snap, err := store.Get(ctx, "org-a", "user-a", "thread-1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if snap.Revision != 1 || len(snap.Files) != 1 || snap.Files[0].ContentHash != "sha256:one" {
		t.Fatalf("snapshot = %+v, want the first writer's file at revision 1 and the refused batch absent", snap)
	}

	other, err := store.Get(ctx, "org-a", "user-b", "thread-1")
	if err != nil {
		t.Fatalf("Get for another user: %v", err)
	}
	if other.Revision != 0 || len(other.Files) != 0 {
		t.Fatalf("another user reads %+v, want an empty thread", other)
	}
}

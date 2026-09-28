//go:build integration

package snapshot

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

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
)

// setupSnapshotDB mirrors lease's own setupLeaseDB (itself mirroring
// capability-core's scope_store_integration_test.go pattern).
func setupSnapshotDB(t *testing.T) *pgxpool.Pool {
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
	for _, migration := range []string{
		"0001_workspace_manifest.up.sql",
		"0002_lease_and_snapshot_store.up.sql",
	} {
		sqlBytes, readErr := os.ReadFile(filepath.Join("..", "..", "migrations", migration))
		if readErr != nil {
			t.Fatalf("read migration %s: %v", migration, readErr)
		}
		if _, applyErr := pool.Exec(ctx, string(sqlBytes)); applyErr != nil {
			t.Fatalf("apply migration %s: %v", migration, applyErr)
		}
	}
	return pool
}

func TestSnapshotStore_CreateAndGet(t *testing.T) {
	pool := setupSnapshotDB(t)
	store, err := NewStore(pool)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	ctx := context.Background()

	snap, err := store.Create(ctx, &lease.Lease{ID: "lease-1"}, "checkpoint-1")
	if err != nil {
		t.Fatalf("Create: %v", err)
	}
	found, err := store.Get(ctx, snap.ID)
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if found.LeaseID != "lease-1" || found.Label != "checkpoint-1" || found.ObjectKey != snap.ObjectKey {
		t.Fatalf("found = %+v, want match for created = %+v", found, snap)
	}
}

func TestSnapshotStore_GetMissingReportsNotFound(t *testing.T) {
	pool := setupSnapshotDB(t)
	store, err := NewStore(pool)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	if _, err := store.Get(context.Background(), "does-not-exist"); err != ErrSnapshotNotFound {
		t.Fatalf("error = %v, want ErrSnapshotNotFound", err)
	}
}

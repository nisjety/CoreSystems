//go:build integration

package workspace

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

// setupWorkspaceDB spins a throwaway Postgres and applies the migration this
// store needs, mirroring internal/lease/lease_integration_test.go's own
// setupLeaseDB.
func setupWorkspaceDB(t *testing.T) string {
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
	sqlBytes, readErr := os.ReadFile(filepath.Join("..", "..", "migrations", "0001_workspace_manifest.up.sql"))
	if readErr != nil {
		t.Fatalf("read migration: %v", readErr)
	}
	if _, applyErr := pool.Exec(ctx, string(sqlBytes)); applyErr != nil {
		t.Fatalf("apply migration: %v", applyErr)
	}
	pool.Close()
	return dsn
}

func newPoolStore(t *testing.T, dsn string) *Store {
	t.Helper()
	pool, err := pgxpool.New(context.Background(), dsn)
	if err != nil {
		t.Fatalf("pool: %v", err)
	}
	t.Cleanup(pool.Close)
	store, err := NewStore(pool)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	return store
}

// TestWorkspaceStore_OverlayShadowsSpaceRowsInTheLayeredManifest is the
// design doc's own named "run overlay shadows the Space" scenario (§3.5.C):
// seed a Space-level row directly, then upsert this run's own overlay for
// the same path plus a second, run-only path, and confirm GetManifest
// returns the overlay's hash for the shared path and both paths overall.
func TestWorkspaceStore_OverlayShadowsSpaceRowsInTheLayeredManifest(t *testing.T) {
	dsn := setupWorkspaceDB(t)
	store := newPoolStore(t, dsn)
	ctx := context.Background()

	seedPool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("seed pool: %v", err)
	}
	defer seedPool.Close()
	if _, err := seedPool.Exec(ctx, `
		INSERT INTO workspace_files (org_id, space_id, run_id, path, content_hash, size_bytes)
		VALUES ('org-a', 'space-a', NULL, 'shared.txt', 'sha256:space-shared', 5),
		       ('org-a', 'space-a', NULL, 'space-only.txt', 'sha256:space-only', 7)
	`); err != nil {
		t.Fatalf("seed space rows: %v", err)
	}

	if err := store.UpsertOverlay(ctx, "org-a", "space-a", "lease-1", []ChangedFile{
		{Path: "shared.txt", ContentHash: "sha256:run-shared", SizeBytes: 9, BaseHash: "sha256:space-shared"},
		{Path: "run-only.txt", ContentHash: "sha256:run-only", SizeBytes: 3},
	}); err != nil {
		t.Fatalf("UpsertOverlay: %v", err)
	}

	entries, err := store.GetManifest(ctx, "org-a", "space-a", "lease-1")
	if err != nil {
		t.Fatalf("GetManifest: %v", err)
	}
	byPath := make(map[string]string, len(entries))
	for _, e := range entries {
		byPath[e.Path] = e.ContentHash
	}
	if len(byPath) != 3 {
		t.Fatalf("entries = %+v, want exactly 3 distinct paths", entries)
	}
	if byPath["shared.txt"] != "sha256:run-shared" {
		t.Fatalf("shared.txt = %q, want the run overlay's hash to shadow the Space row", byPath["shared.txt"])
	}
	if byPath["space-only.txt"] != "sha256:space-only" {
		t.Fatalf("space-only.txt = %q, want the untouched Space row", byPath["space-only.txt"])
	}
	if byPath["run-only.txt"] != "sha256:run-only" {
		t.Fatalf("run-only.txt = %q, want the new overlay row", byPath["run-only.txt"])
	}

	// A different run's manifest must not see lease-1's overlay at all.
	otherRunEntries, err := store.GetManifest(ctx, "org-a", "space-a", "lease-2")
	if err != nil {
		t.Fatalf("GetManifest (other run): %v", err)
	}
	for _, e := range otherRunEntries {
		if e.Path == "run-only.txt" {
			t.Fatalf("a different run must not see lease-1's own overlay: %+v", otherRunEntries)
		}
	}
}

// TestWorkspaceStore_UpsertOverlayIsIdempotent confirms a second upsert for
// the same path replaces rather than duplicates the row (relies on the
// migration's own workspace_files_identity_uq index).
func TestWorkspaceStore_UpsertOverlayIsIdempotent(t *testing.T) {
	dsn := setupWorkspaceDB(t)
	store := newPoolStore(t, dsn)
	ctx := context.Background()

	for _, hash := range []string{"sha256:v1", "sha256:v2"} {
		if err := store.UpsertOverlay(ctx, "org-a", "space-a", "lease-1", []ChangedFile{
			{Path: "a.txt", ContentHash: hash, SizeBytes: 1},
		}); err != nil {
			t.Fatalf("UpsertOverlay(%s): %v", hash, err)
		}
	}

	entries, err := store.GetManifest(ctx, "org-a", "space-a", "lease-1")
	if err != nil {
		t.Fatalf("GetManifest: %v", err)
	}
	if len(entries) != 1 || entries[0].ContentHash != "sha256:v2" {
		t.Fatalf("entries = %+v, want exactly one row at the latest hash", entries)
	}
}

//go:build integration

package workspace

import (
	"context"
	"os"
	"path/filepath"
	"reflect"
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

// spaceRowHash reads a single Space-level (run_id IS NULL) row's
// content_hash directly, for asserting Promote's own effect on the Space
// independent of the layered GetManifest view.
func spaceRowHash(t *testing.T, ctx context.Context, dsn, orgID, spaceID, path string) (string, bool) {
	t.Helper()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("read pool: %v", err)
	}
	defer pool.Close()
	var hash string
	err = pool.QueryRow(ctx, `
		SELECT content_hash FROM workspace_files
		WHERE org_id = $1 AND space_id = $2 AND run_id IS NULL AND path = $3
	`, orgID, spaceID, path).Scan(&hash)
	if err != nil {
		return "", false
	}
	return hash, true
}

// TestWorkspaceStore_PromoteMergesNonConflictingPathsIndependently is the
// design doc's own named scenario (§4, §7): a run's overlay has a brand-new
// path (no prior Space row at all) alongside a path whose base_hash is
// stale — the new path merges, the stale one is reported as conflicting,
// and neither blocks the other.
func TestWorkspaceStore_PromoteMergesNonConflictingPathsIndependently(t *testing.T) {
	dsn := setupWorkspaceDB(t)
	ctx := context.Background()
	store := newPoolStore(t, dsn)

	seedPool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("seed pool: %v", err)
	}
	defer seedPool.Close()
	if _, err := seedPool.Exec(ctx, `
		INSERT INTO workspace_files (org_id, space_id, run_id, path, content_hash, size_bytes)
		VALUES ('org-a', 'space-a', NULL, 'stale.txt', 'sha256:current', 4)
	`); err != nil {
		t.Fatalf("seed space row: %v", err)
	}
	if err := store.UpsertOverlay(ctx, "org-a", "space-a", "lease-1", []ChangedFile{
		{Path: "new.txt", ContentHash: "sha256:brand-new", SizeBytes: 3},
		{Path: "stale.txt", ContentHash: "sha256:overlay-attempt", SizeBytes: 8, BaseHash: "sha256:stale-base"},
	}); err != nil {
		t.Fatalf("UpsertOverlay: %v", err)
	}

	conflicts, err := store.Promote(ctx, "org-a", "space-a", "lease-1")
	if err != nil {
		t.Fatalf("Promote: %v", err)
	}
	if !reflect.DeepEqual(conflicts, []string{"stale.txt"}) {
		t.Fatalf("conflicts = %v, want [stale.txt]", conflicts)
	}

	if hash, ok := spaceRowHash(t, ctx, dsn, "org-a", "space-a", "new.txt"); !ok || hash != "sha256:brand-new" {
		t.Fatalf("new.txt Space row = (%q, %v), want the merged new content", hash, ok)
	}
	if hash, ok := spaceRowHash(t, ctx, dsn, "org-a", "space-a", "stale.txt"); !ok || hash != "sha256:current" {
		t.Fatalf("stale.txt Space row = (%q, %v), want the ORIGINAL content untouched by the conflicting attempt", hash, ok)
	}
}

// TestWorkspaceStore_PromoteReportsConflictWithoutOverwriting is the design
// doc's own named scenario (§4, §7): a path whose base_hash no longer
// matches the Space's current content (someone else's change landed first)
// must be reported as conflicting, and the Space's existing content must
// survive untouched — never silently overwritten by the losing attempt.
func TestWorkspaceStore_PromoteReportsConflictWithoutOverwriting(t *testing.T) {
	dsn := setupWorkspaceDB(t)
	ctx := context.Background()
	store := newPoolStore(t, dsn)

	seedPool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("seed pool: %v", err)
	}
	defer seedPool.Close()
	if _, err := seedPool.Exec(ctx, `
		INSERT INTO workspace_files (org_id, space_id, run_id, path, content_hash, size_bytes)
		VALUES ('org-a', 'space-a', NULL, 'contested.txt', 'sha256:winner', 6)
	`); err != nil {
		t.Fatalf("seed space row: %v", err)
	}
	if err := store.UpsertOverlay(ctx, "org-a", "space-a", "lease-loser", []ChangedFile{
		{Path: "contested.txt", ContentHash: "sha256:loser", SizeBytes: 9, BaseHash: "sha256:original"},
	}); err != nil {
		t.Fatalf("UpsertOverlay: %v", err)
	}

	conflicts, err := store.Promote(ctx, "org-a", "space-a", "lease-loser")
	if err != nil {
		t.Fatalf("Promote: %v", err)
	}
	if !reflect.DeepEqual(conflicts, []string{"contested.txt"}) {
		t.Fatalf("conflicts = %v, want [contested.txt]", conflicts)
	}
	if hash, ok := spaceRowHash(t, ctx, dsn, "org-a", "space-a", "contested.txt"); !ok || hash != "sha256:winner" {
		t.Fatalf("contested.txt Space row = (%q, %v), want the winner's content untouched", hash, ok)
	}
}

// TestWorkspaceStore_PromoteMergesAChangedPathViaUpdateAlongsideABrandNewOne
// is an adversarial-review-identified coverage gap, closed: every other
// Promote integration test's SUCCESSFUL merge is either a brand-new path
// (INSERT, no prior Space row) or an already-merged re-promote (the
// EXCLUDED.content_hash disjunct). This is the one real-Postgres proof of
// the REMAINING branch — a pre-existing Space row whose content_hash
// matches the overlay's base_hash, updated to genuinely NEW content via
// the UPDATE side of the same statement — exercised in the SAME
// PromoteWorkspace call as a brand-new path, so both branches are proven
// to merge independently within one call, not just in isolation.
func TestWorkspaceStore_PromoteMergesAChangedPathViaUpdateAlongsideABrandNewOne(t *testing.T) {
	dsn := setupWorkspaceDB(t)
	ctx := context.Background()
	store := newPoolStore(t, dsn)

	seedPool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("seed pool: %v", err)
	}
	defer seedPool.Close()
	if _, err := seedPool.Exec(ctx, `
		INSERT INTO workspace_files (org_id, space_id, run_id, path, content_hash, size_bytes)
		VALUES ('org-a', 'space-a', NULL, 'existing.txt', 'sha256:v1', 4)
	`); err != nil {
		t.Fatalf("seed space row: %v", err)
	}
	if err := store.UpsertOverlay(ctx, "org-a", "space-a", "lease-1", []ChangedFile{
		{Path: "existing.txt", ContentHash: "sha256:v2", SizeBytes: 6, BaseHash: "sha256:v1"},
		{Path: "brand-new.txt", ContentHash: "sha256:new", SizeBytes: 3},
	}); err != nil {
		t.Fatalf("UpsertOverlay: %v", err)
	}

	conflicts, err := store.Promote(ctx, "org-a", "space-a", "lease-1")
	if err != nil {
		t.Fatalf("Promote: %v", err)
	}
	if len(conflicts) != 0 {
		t.Fatalf("conflicts = %v, want none", conflicts)
	}
	if hash, ok := spaceRowHash(t, ctx, dsn, "org-a", "space-a", "existing.txt"); !ok || hash != "sha256:v2" {
		t.Fatalf("existing.txt Space row = (%q, %v), want the UPDATE branch's new content", hash, ok)
	}
	if hash, ok := spaceRowHash(t, ctx, dsn, "org-a", "space-a", "brand-new.txt"); !ok || hash != "sha256:new" {
		t.Fatalf("brand-new.txt Space row = (%q, %v), want the INSERT branch's content", hash, ok)
	}
}

// TestWorkspaceStore_PromoteIsIdempotentForAnAlreadyMergedPath is the exact
// subtlety a naive base_hash-only compare-and-swap gets wrong: after a
// successful merge, the Space's hash no longer equals base_hash (it now
// equals the run's OWN new content), so a second Promote call for the same
// unchanged overlay must not report a false conflict.
func TestWorkspaceStore_PromoteIsIdempotentForAnAlreadyMergedPath(t *testing.T) {
	dsn := setupWorkspaceDB(t)
	ctx := context.Background()
	store := newPoolStore(t, dsn)

	if err := store.UpsertOverlay(ctx, "org-a", "space-a", "lease-1", []ChangedFile{
		{Path: "a.txt", ContentHash: "sha256:new", SizeBytes: 2},
	}); err != nil {
		t.Fatalf("UpsertOverlay: %v", err)
	}

	first, err := store.Promote(ctx, "org-a", "space-a", "lease-1")
	if err != nil {
		t.Fatalf("first Promote: %v", err)
	}
	if len(first) != 0 {
		t.Fatalf("first Promote conflicts = %v, want none", first)
	}

	second, err := store.Promote(ctx, "org-a", "space-a", "lease-1")
	if err != nil {
		t.Fatalf("second Promote: %v", err)
	}
	if len(second) != 0 {
		t.Fatalf("second Promote (same unchanged overlay) conflicts = %v, want none -- a re-promote must be a safe no-op, not a false conflict", second)
	}
	if hash, ok := spaceRowHash(t, ctx, dsn, "org-a", "space-a", "a.txt"); !ok || hash != "sha256:new" {
		t.Fatalf("a.txt Space row = (%q, %v), want the merged content still in place", hash, ok)
	}
}

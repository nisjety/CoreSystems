//go:build integration

package workspace

import (
	"context"
	"reflect"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"
)

func seedSpaceRows(t *testing.T, dsn, values string) {
	t.Helper()
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("seed pool: %v", err)
	}
	defer pool.Close()
	if _, err := pool.Exec(ctx, `INSERT INTO workspace_files (org_id, space_id, run_id, path, content_hash, size_bytes) VALUES `+values); err != nil {
		t.Fatalf("seed space rows: %v", err)
	}
}

func manifestMap(t *testing.T, store *Store, runID string) map[string]string {
	t.Helper()
	entries, err := store.GetManifest(context.Background(), "org-a", "space-a", runID)
	if err != nil {
		t.Fatalf("GetManifest: %v", err)
	}
	out := make(map[string]string, len(entries))
	for _, e := range entries {
		out[e.Path] = e.ContentHash
	}
	return out
}

func TestWorkspaceStore_TombstoneHidesThePathAndRecreateUnTombstones(t *testing.T) {
	dsn := setupWorkspaceDB(t)
	store := newPoolStore(t, dsn)
	ctx := context.Background()
	seedSpaceRows(t, dsn, `('org-a', 'space-a', NULL, 'gone.txt', 'sha256:gone', 4), ('org-a', 'space-a', NULL, 'kept.txt', 'sha256:kept', 4)`)

	if err := store.UpsertTombstones(ctx, "org-a", "space-a", "lease-1", []DeletedPath{{Path: "gone.txt", BaseHash: "sha256:gone"}}); err != nil {
		t.Fatalf("UpsertTombstones: %v", err)
	}
	if got := manifestMap(t, store, "lease-1"); !reflect.DeepEqual(got, map[string]string{"kept.txt": "sha256:kept"}) {
		t.Fatalf("run manifest = %v, want gone.txt hidden", got)
	}
	// Another run, and the Space itself, still see the file.
	if got := manifestMap(t, store, "lease-2"); got["gone.txt"] != "sha256:gone" {
		t.Fatalf("another run's manifest = %v, want gone.txt visible", got)
	}

	if err := store.UpsertOverlay(ctx, "org-a", "space-a", "lease-1", []ChangedFile{
		{Path: "gone.txt", ContentHash: "sha256:again", SizeBytes: 5, BaseHash: "sha256:gone"},
	}); err != nil {
		t.Fatalf("UpsertOverlay (recreate): %v", err)
	}
	if got := manifestMap(t, store, "lease-1"); got["gone.txt"] != "sha256:again" {
		t.Fatalf("manifest after recreate = %v, want the recreated file visible", got)
	}
}

func TestWorkspaceStore_PromoteDeletesTheSpaceRowWhenTheHashMatchesAndIsIdempotent(t *testing.T) {
	dsn := setupWorkspaceDB(t)
	store := newPoolStore(t, dsn)
	ctx := context.Background()
	seedSpaceRows(t, dsn, `('org-a', 'space-a', NULL, 'gone.txt', 'sha256:gone', 4)`)
	if err := store.UpsertTombstones(ctx, "org-a", "space-a", "lease-1", []DeletedPath{{Path: "gone.txt", BaseHash: "sha256:gone"}}); err != nil {
		t.Fatalf("UpsertTombstones: %v", err)
	}

	for pass := 1; pass <= 2; pass++ {
		conflicts, err := store.Promote(ctx, "org-a", "space-a", "lease-1")
		if err != nil {
			t.Fatalf("Promote (pass %d): %v", pass, err)
		}
		if len(conflicts) != 0 {
			t.Fatalf("Promote (pass %d) conflicts = %v, want none (the replay must be a no-op)", pass, conflicts)
		}
		if hash, ok := spaceRowHash(t, ctx, dsn, "org-a", "space-a", "gone.txt"); ok {
			t.Fatalf("pass %d: gone.txt Space row = %q, want it deleted", pass, hash)
		}
	}
}

func TestWorkspaceStore_PromoteReportsATombstoneConflictWhenTheSpaceRowChanged(t *testing.T) {
	dsn := setupWorkspaceDB(t)
	store := newPoolStore(t, dsn)
	ctx := context.Background()
	seedSpaceRows(t, dsn, `('org-a', 'space-a', NULL, 'changed.txt', 'sha256:someone-elses', 6), ('org-a', 'space-a', NULL, 'clean.txt', 'sha256:clean', 4)`)
	if err := store.UpsertTombstones(ctx, "org-a", "space-a", "lease-1", []DeletedPath{
		{Path: "changed.txt", BaseHash: "sha256:what-the-run-saw"},
		{Path: "clean.txt", BaseHash: "sha256:clean"},
	}); err != nil {
		t.Fatalf("UpsertTombstones: %v", err)
	}

	conflicts, err := store.Promote(ctx, "org-a", "space-a", "lease-1")
	if err != nil {
		t.Fatalf("Promote: %v", err)
	}
	if !reflect.DeepEqual(conflicts, []string{"changed.txt"}) {
		t.Fatalf("conflicts = %v, want [changed.txt]", conflicts)
	}
	if hash, ok := spaceRowHash(t, ctx, dsn, "org-a", "space-a", "changed.txt"); !ok || hash != "sha256:someone-elses" {
		t.Fatalf("changed.txt Space row = (%q, %v), want the other writer's content untouched", hash, ok)
	}
	if _, ok := spaceRowHash(t, ctx, dsn, "org-a", "space-a", "clean.txt"); ok {
		t.Fatal("clean.txt Space row survived, want it deleted independently of the conflict")
	}
}

func TestWorkspaceStore_TombstoneForAPathTheRunCreatedDropsItsOverlayRow(t *testing.T) {
	dsn := setupWorkspaceDB(t)
	store := newPoolStore(t, dsn)
	ctx := context.Background()
	if err := store.UpsertOverlay(ctx, "org-a", "space-a", "lease-1", []ChangedFile{{Path: "scratch.txt", ContentHash: "sha256:s", SizeBytes: 1}}); err != nil {
		t.Fatalf("UpsertOverlay: %v", err)
	}
	if err := store.UpsertTombstones(ctx, "org-a", "space-a", "lease-1", []DeletedPath{{Path: "scratch.txt"}}); err != nil {
		t.Fatalf("UpsertTombstones: %v", err)
	}
	if got := manifestMap(t, store, "lease-1"); len(got) != 0 {
		t.Fatalf("manifest = %v, want empty", got)
	}
	if conflicts, err := store.Promote(ctx, "org-a", "space-a", "lease-1"); err != nil || len(conflicts) != 0 {
		t.Fatalf("Promote = (%v, %v), want clean", conflicts, err)
	}
	if _, ok := spaceRowHash(t, ctx, dsn, "org-a", "space-a", "scratch.txt"); ok {
		t.Fatal("scratch.txt reached the Space, want nothing promoted")
	}
}

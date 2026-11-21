//go:build integration

package threadworkspace

import (
	"context"
	"slices"
	"testing"
	"time"
)

// O-13 on the production SQL: ListOlderThan pages in primary-key order and honours
// the grace window, and EraseOrphan locks the header, keeps a workspace younger than
// the cutoff, erases every owner's rows of a gone thread, and writes the tombstone.
//
//	go test -tags integration -run 'TestOrphan' ./internal/threadworkspace/...

func TestOrphanListOlderThanPagesInKeyOrderAndHonoursTheGrace(t *testing.T) {
	pool := setupThreadDB(t)
	ctx := context.Background()
	store, err := NewStore(pool)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	eraser, err := NewEraser(pool)
	if err != nil {
		t.Fatalf("NewEraser: %v", err)
	}
	limits := Limits{MaxFiles: 10, MaxBytes: 1 << 20}
	file := []Change{{Path: "notes.txt", ContentHash: "sha256:" + hex64('a'), SizeBytes: 5}}
	for _, k := range []ThreadKey{
		{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_a"},
		{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_b"},
		{OrgID: "org_2", OwnerID: "user_1", ThreadID: "thr_c"},
		{OrgID: "org_2", OwnerID: "user_1", ThreadID: "thr_new"},
	} {
		if _, err := store.Apply(ctx, k.OrgID, k.OwnerID, k.ThreadID, file, nil, limits); err != nil {
			t.Fatalf("apply %v: %v", k, err)
		}
	}
	if _, err := pool.Exec(ctx, `UPDATE thread_workspaces SET created_at = now() - interval '48 hours' WHERE thread_id <> 'thr_new'`); err != nil {
		t.Fatalf("age workspaces: %v", err)
	}

	cutoff := time.Now().Add(-DefaultOrphanGrace)
	first, err := eraser.ListOlderThan(ctx, cutoff, ThreadKey{}, 2)
	if err != nil {
		t.Fatalf("first page: %v", err)
	}
	want := []ThreadKey{
		{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_a"},
		{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_b"},
	}
	if !slices.Equal(first, want) {
		t.Fatalf("first page = %v, want %v", first, want)
	}
	second, err := eraser.ListOlderThan(ctx, cutoff, first[len(first)-1], 2)
	if err != nil {
		t.Fatalf("second page: %v", err)
	}
	if len(second) != 1 || second[0].ThreadID != "thr_c" {
		t.Fatalf("second page = %v, want only thr_c (thr_new is inside the grace window)", second)
	}
}

func TestOrphanEraseKeepsAYoungWorkspaceAndErasesAnOldOneWithItsTombstone(t *testing.T) {
	pool := setupThreadDB(t)
	ctx := context.Background()
	store, err := NewStore(pool)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	eraser, err := NewEraser(pool)
	if err != nil {
		t.Fatalf("NewEraser: %v", err)
	}
	limits := Limits{MaxFiles: 10, MaxBytes: 1 << 20}
	file := []Change{{Path: "notes.txt", ContentHash: "sha256:" + hex64('a'), SizeBytes: 5}}
	old := ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_old"}
	young := ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_young"}
	for _, k := range []ThreadKey{old, young} {
		if _, err := store.Apply(ctx, k.OrgID, k.OwnerID, k.ThreadID, file, nil, limits); err != nil {
			t.Fatalf("apply %v: %v", k, err)
		}
	}
	if _, err := pool.Exec(ctx, `UPDATE thread_workspaces SET created_at = now() - interval '48 hours' WHERE thread_id = 'thr_old'`); err != nil {
		t.Fatalf("age: %v", err)
	}
	cutoff := time.Now().Add(-DefaultOrphanGrace)

	if ok, _, err := eraser.EraseOrphan(ctx, young, cutoff); err != nil || ok {
		t.Fatalf("EraseOrphan(young) = (%v, %v), want it kept", ok, err)
	}
	if snap, _ := store.Get(ctx, young.OrgID, young.OwnerID, young.ThreadID); len(snap.Files) != 1 {
		t.Fatal("a workspace inside the grace window lost its files")
	}
	ok, files, err := eraser.EraseOrphan(ctx, old, cutoff)
	if err != nil || !ok || files != 1 {
		t.Fatalf("EraseOrphan(old) = (%v, %d, %v), want erased with one file", ok, files, err)
	}
	if n := tombstoneCount(t, pool, old); n != 1 {
		t.Fatalf("tombstones for the orphan = %d, want 1", n)
	}
	if ok, _, err := eraser.EraseOrphan(ctx, old, cutoff); err != nil || ok {
		t.Fatalf("EraseOrphan again = (%v, %v), want a no-op", ok, err)
	}
}

// TestOrphanEraseTakesEveryOwnerOfAGoneThread: the shared-owner refusal that protects
// a live thread does not apply to an orphan, so each owner's rows go, one call each.
func TestOrphanEraseTakesEveryOwnerOfAGoneThread(t *testing.T) {
	pool := setupThreadDB(t)
	ctx := context.Background()
	store, err := NewStore(pool)
	if err != nil {
		t.Fatalf("NewStore: %v", err)
	}
	eraser, err := NewEraser(pool)
	if err != nil {
		t.Fatalf("NewEraser: %v", err)
	}
	limits := Limits{MaxFiles: 10, MaxBytes: 1 << 20}
	file := []Change{{Path: "notes.txt", ContentHash: "sha256:" + hex64('a'), SizeBytes: 5}}
	for _, owner := range []string{"user_1", "user_2"} {
		if _, err := store.Apply(ctx, "org_1", owner, "thr_s", file, nil, limits); err != nil {
			t.Fatalf("apply %s: %v", owner, err)
		}
	}
	if _, err := pool.Exec(ctx, `UPDATE thread_workspaces SET created_at = now() - interval '48 hours'`); err != nil {
		t.Fatalf("age: %v", err)
	}
	cutoff := time.Now().Add(-DefaultOrphanGrace)
	for _, owner := range []string{"user_1", "user_2"} {
		ok, _, err := eraser.EraseOrphan(ctx, ThreadKey{OrgID: "org_1", OwnerID: owner, ThreadID: "thr_s"}, cutoff)
		if err != nil || !ok {
			t.Fatalf("EraseOrphan(%s) = (%v, %v), want erased", owner, ok, err)
		}
	}
	var left int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM thread_workspaces WHERE thread_id = 'thr_s'`).Scan(&left); err != nil || left != 0 {
		t.Fatalf("headers left = %d (%v), want 0", left, err)
	}
}

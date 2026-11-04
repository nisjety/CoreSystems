//go:build integration

package threadworkspace

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// TestEraseIdleKeepsAThreadUsedAfterTheSweepChoseIt: the idle check is made under
// the header lock, so a read that lands between the listing and the erase keeps
// the thread's rows. Erasing is scoped to one owner's thread.
func TestEraseIdleKeepsAThreadUsedAfterTheSweepChoseIt(t *testing.T) {
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

	for _, thread := range []string{"thr_old", "thr_new"} {
		if _, err := store.Apply(ctx, "org_1", "user_1", thread, file, nil, limits); err != nil {
			t.Fatalf("apply %s: %v", thread, err)
		}
	}
	if _, err := store.Apply(ctx, "org_1", "user_2", "thr_old", file, nil, limits); err != nil {
		t.Fatalf("apply other owner: %v", err)
	}
	if _, err := pool.Exec(ctx, `UPDATE thread_workspaces SET last_used_at = now() - interval '40 days' WHERE thread_id = 'thr_old' AND owner_id = 'user_1'`); err != nil {
		t.Fatalf("age thr_old: %v", err)
	}

	cutoff := time.Now().Add(-30 * 24 * time.Hour)
	idle, err := eraser.ListIdle(ctx, cutoff, sweepBatch)
	if err != nil {
		t.Fatalf("ListIdle: %v", err)
	}
	if len(idle) != 1 || idle[0] != (ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_old"}) {
		t.Fatalf("idle = %v, want only user_1's thr_old", idle)
	}

	// A read lands after the listing: it refreshes last_used_at, so the erase keeps the thread.
	if _, err := store.Get(ctx, "org_1", "user_1", "thr_old"); err != nil {
		t.Fatalf("touch thr_old: %v", err)
	}
	erased, _, err := eraser.EraseIdle(ctx, idle[0], cutoff)
	if err != nil || erased {
		t.Fatalf("EraseIdle after a read = (%v, %v); want the thread kept", erased, err)
	}
	if snap, _ := store.Get(ctx, "org_1", "user_1", "thr_old"); len(snap.Files) != 1 {
		t.Fatalf("thr_old lost its file after a read between list and erase")
	}

	if _, err := pool.Exec(ctx, `UPDATE thread_workspaces SET last_used_at = now() - interval '40 days' WHERE thread_id = 'thr_old' AND owner_id = 'user_1'`); err != nil {
		t.Fatalf("age thr_old again: %v", err)
	}
	// user_2 also holds thr_old, and the blob prefix carries no owner, so the erase is
	// refused and both owners keep their files (ErrThreadShared).
	erased, files, err := eraser.EraseIdle(ctx, idle[0], cutoff)
	if !errors.Is(err, ErrThreadShared) || erased {
		t.Fatalf("EraseIdle of a shared thread id = (%v, %d, %v); want ErrThreadShared", erased, files, err)
	}
	for _, owner := range []string{"user_1", "user_2"} {
		if snap, _ := store.Get(ctx, "org_1", owner, "thr_old"); len(snap.Files) != 1 {
			t.Fatalf("a refused erase of the shared thr_old removed %s's files", owner)
		}
	}

	// Once user_2's rows are gone the idle thread is erased.
	if _, err := eraser.Erase(ctx, "org_1", "user_2", "thr_old"); !errors.Is(err, ErrThreadShared) {
		t.Fatalf("Erase of user_2's thr_old = %v; want ErrThreadShared while user_1 holds it", err)
	}
	if _, err := pool.Exec(ctx, `DELETE FROM thread_workspace_files WHERE owner_id = 'user_2'`); err != nil {
		t.Fatalf("drop user_2's files: %v", err)
	}
	if _, err := pool.Exec(ctx, `DELETE FROM thread_workspaces WHERE owner_id = 'user_2'`); err != nil {
		t.Fatalf("drop user_2's header: %v", err)
	}
	if _, err := pool.Exec(ctx, `UPDATE thread_workspaces SET last_used_at = now() - interval '40 days' WHERE thread_id = 'thr_old' AND owner_id = 'user_1'`); err != nil {
		t.Fatalf("age thr_old a third time: %v", err)
	}
	erased, files, err = eraser.EraseIdle(ctx, idle[0], cutoff)
	if err != nil || !erased || files != 1 {
		t.Fatalf("EraseIdle while idle = (%v, %d, %v); want one file erased", erased, files, err)
	}
	if snap, _ := store.Get(ctx, "org_1", "user_1", "thr_new"); len(snap.Files) != 1 {
		t.Fatalf("erasing thr_old removed the sibling thread")
	}

	if files, err := eraser.Erase(ctx, "org_1", "user_1", "thr_new"); err != nil || files != 1 {
		t.Fatalf("Erase thr_new = (%d, %v); want one file", files, err)
	}
	if files, err := eraser.Erase(ctx, "org_1", "user_1", "thr_new"); err != nil || files != 0 {
		t.Fatalf("second Erase thr_new = (%d, %v); want a no-op", files, err)
	}
	if files, err := eraser.Erase(ctx, "org_1", "user_1", "thr_shared_elsewhere"); err != nil || files != 0 {
		t.Fatalf("Erase of an unknown thread = (%d, %v); want a no-op", files, err)
	}
}

// TestEraseStaleLeasesKeepsOnlyLiveAndRecentThreadLeases runs the SQL the retention
// pass uses against the lease table. A thread lease that is destroyed or expired,
// and was created before the cutoff, goes; a live one, a recent one and every
// agent-scoped lease stay.
func TestEraseStaleLeasesKeepsOnlyLiveAndRecentThreadLeases(t *testing.T) {
	pool := setupThreadDB(t)
	applyLeaseMigrations(t, pool)
	ctx := context.Background()
	eraser, err := NewEraser(pool)
	if err != nil {
		t.Fatalf("NewEraser: %v", err)
	}

	const scratch, destroyed int32 = 1, 4
	const day = 24 * time.Hour
	now := time.Now().UTC()
	insertLease(t, pool, "lease_thread_expired_old", "thread", scratch, now.Add(-40*day), now.Add(-41*day))
	insertLease(t, pool, "lease_thread_destroyed_old", "thread", destroyed, now.Add(-35*day), now.Add(-40*day))
	insertLease(t, pool, "lease_thread_expired_recent", "thread", scratch, now.Add(-5*day), now.Add(-6*day))
	insertLease(t, pool, "lease_thread_destroyed_recent", "thread", destroyed, now.Add(-time.Hour), now.Add(-2*day))
	insertLease(t, pool, "lease_thread_live_old", "thread", scratch, now.Add(time.Hour), now.Add(-40*day))
	insertLease(t, pool, "lease_agent_expired_old", "agent", scratch, now.Add(-40*day), now.Add(-41*day))
	insertLease(t, pool, "lease_agent_destroyed_old", "agent", destroyed, now.Add(-40*day), now.Add(-41*day))

	if _, err := NewSweeper(eraser, &fakeBlobs{}, 30*day).sweep(ctx, now); err != nil {
		t.Fatalf("sweep: %v", err)
	}

	rows, err := pool.Query(ctx, `SELECT id FROM leases ORDER BY id`)
	if err != nil {
		t.Fatalf("list leases: %v", err)
	}
	defer rows.Close()
	var kept []string
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			t.Fatalf("scan lease: %v", err)
		}
		kept = append(kept, id)
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("list leases: %v", err)
	}
	want := []string{
		"lease_agent_destroyed_old",
		"lease_agent_expired_old",
		"lease_thread_destroyed_recent",
		"lease_thread_expired_recent",
		"lease_thread_live_old",
	}
	if !slices.Equal(kept, want) {
		t.Fatalf("leases kept = %v, want %v", kept, want)
	}
}

// applyLeaseMigrations creates the lease tables the retention pass deletes from,
// which setupThreadDB does not apply.
func applyLeaseMigrations(t *testing.T, pool *pgxpool.Pool) {
	t.Helper()
	for _, name := range []string{"0002_lease_and_snapshot_store.up.sql", "0006_thread_lease_unique.up.sql"} {
		sqlBytes, err := os.ReadFile(filepath.Join("..", "..", "migrations", name))
		if err != nil {
			t.Fatalf("read %s: %v", name, err)
		}
		if _, err := pool.Exec(context.Background(), string(sqlBytes)); err != nil {
			t.Fatalf("apply %s: %v", name, err)
		}
	}
}

// insertLease writes one lease row directly, so the test sets its state and both
// timestamps.
func insertLease(t *testing.T, pool *pgxpool.Pool, id, scopeType string, state int32, expiresAt, createdAt time.Time) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO leases (id, scope_id, scope_type, org_id, owner_id, endpoint, state, expires_at, created_at)
		VALUES ($1, $1, $2, 'org_1', 'user_1', $3, $4, $5, $6)
	`, id, scopeType, "sandbox://"+id, state, expiresAt, createdAt); err != nil {
		t.Fatalf("insert lease %s: %v", id, err)
	}
}

func hex64(c byte) string {
	out := make([]byte, 64)
	for i := range out {
		out[i] = c
	}
	return string(out)
}

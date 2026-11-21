//go:build integration

package threadworkspace

import (
	"context"
	"errors"
	"slices"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// F-f06-02 on the production SQL: the tombstone is written with the rows, a failed
// blob removal keeps it and a later pass closes it, and a thread id that is live
// again keeps its tombstone out of the retry.
//
//	go test -tags integration -run 'TestTombstone|TestEraseWritesTheTombstone|TestBlobFailureKeepsTheTombstone|TestPendingErasure' ./internal/threadworkspace/...

// tombstoneCount counts the tombstones of one thread key.
func tombstoneCount(t *testing.T, pool *pgxpool.Pool, key ThreadKey) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(context.Background(), `
		SELECT count(*) FROM thread_workspace_erasures
		WHERE org_id = $1 AND owner_id = $2 AND thread_id = $3
	`, key.OrgID, key.OwnerID, key.ThreadID).Scan(&n); err != nil {
		t.Fatalf("count tombstones: %v", err)
	}
	return n
}

// ageTombstones moves every tombstone's request time back, so the retry drain may
// take it (it waits blobErasureSettle).
func ageTombstones(t *testing.T, pool *pgxpool.Pool, by time.Duration) {
	t.Helper()
	if _, err := pool.Exec(context.Background(),
		`UPDATE thread_workspace_erasures SET requested_at = now() - make_interval(secs => $1)`, by.Seconds()); err != nil {
		t.Fatalf("age tombstones: %v", err)
	}
}

// TestEraseWritesTheTombstoneWithTheRows: an erase deletes the rows and writes the
// tombstone in one transaction, and a thread that held nothing gets one too, so
// its blob removal is still recorded.
func TestEraseWritesTheTombstoneWithTheRows(t *testing.T) {
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
	if _, err := store.Apply(ctx, "org_1", "user_1", "thr_a", file, nil, limits); err != nil {
		t.Fatalf("apply: %v", err)
	}

	key := ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_a"}
	if files, err := eraser.Erase(ctx, key.OrgID, key.OwnerID, key.ThreadID); err != nil || files != 1 {
		t.Fatalf("Erase = (%d, %v), want one file", files, err)
	}
	if n := tombstoneCount(t, pool, key); n != 1 {
		t.Fatalf("tombstones for the erased thread = %d, want 1", n)
	}
	var attempts int64
	var last *time.Time
	if err := pool.QueryRow(ctx, `SELECT attempts, last_attempt_at FROM thread_workspace_erasures WHERE thread_id = 'thr_a'`).Scan(&attempts, &last); err != nil {
		t.Fatalf("read tombstone: %v", err)
	}
	if attempts != 0 || last != nil {
		t.Fatalf("a new tombstone has attempts = %d, last attempt = %v; want 0 and none", attempts, last)
	}

	empty := ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_none"}
	if _, err := eraser.Erase(ctx, empty.OrgID, empty.OwnerID, empty.ThreadID); err != nil {
		t.Fatalf("Erase of a thread with no rows: %v", err)
	}
	if n := tombstoneCount(t, pool, empty); n != 1 {
		t.Fatalf("tombstones for an empty thread = %d, want 1", n)
	}
}

// TestEraseOfASharedThreadWritesNoTombstone: a refused erase removes nothing, so it
// records nothing, and the other owner's rows stay.
func TestEraseOfASharedThreadWritesNoTombstone(t *testing.T) {
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

	if _, err := eraser.Erase(ctx, "org_1", "user_1", "thr_s"); !errors.Is(err, ErrThreadShared) {
		t.Fatalf("Erase of a shared thread = %v, want ErrThreadShared", err)
	}
	if n := tombstoneCount(t, pool, ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_s"}); n != 0 {
		t.Fatalf("a refused erase wrote %d tombstones", n)
	}
	if snap, _ := store.Get(ctx, "org_1", "user_1", "thr_s"); len(snap.Files) != 1 {
		t.Fatal("a refused erase removed the owner's files")
	}
}

// TestBlobFailureKeepsTheTombstoneUntilALaterSweepCompletesIt: an erase's blob
// removal fails, so the tombstone stays. A pass records the attempt; a later pass
// removes the blobs and closes the tombstone.
func TestBlobFailureKeepsTheTombstoneUntilALaterSweepCompletesIt(t *testing.T) {
	pool := setupThreadDB(t)
	applyLeaseMigrations(t, pool)
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
	if _, err := store.Apply(ctx, "org_1", "user_1", "thr_b", file, nil, limits); err != nil {
		t.Fatalf("apply: %v", err)
	}
	key := ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_b"}
	if _, err := eraser.Erase(ctx, key.OrgID, key.OwnerID, key.ThreadID); err != nil {
		t.Fatalf("Erase: %v", err)
	}
	// The erase's own blob removal failed, so nothing closed the tombstone.
	ageTombstones(t, pool, 5*time.Minute)

	failing := &fakeBlobs{err: errors.New("object store unavailable")}
	if _, err := NewSweeper(eraser, failing, 30*24*time.Hour).sweep(ctx, time.Now()); err != nil {
		t.Fatalf("sweep with a failing object store: %v", err)
	}
	var attempts int64
	if err := pool.QueryRow(ctx, `SELECT attempts FROM thread_workspace_erasures WHERE thread_id = 'thr_b'`).Scan(&attempts); err != nil {
		t.Fatalf("read attempts: %v", err)
	}
	if attempts != 1 || tombstoneCount(t, pool, key) != 1 {
		t.Fatalf("after a failed retry: attempts = %d, tombstones kept = %v; want 1 and kept", attempts, tombstoneCount(t, pool, key) == 1)
	}

	ok := &fakeBlobs{}
	if _, err := NewSweeper(eraser, ok, 30*24*time.Hour).sweep(ctx, time.Now()); err != nil {
		t.Fatalf("sweep with a working object store: %v", err)
	}
	if want := []string{"org_1/thr_b"}; !slices.Equal(ok.removed, want) {
		t.Fatalf("blobs removed on the later pass = %v, want %v", ok.removed, want)
	}
	if n := tombstoneCount(t, pool, key); n != 0 {
		t.Fatalf("the later pass left %d tombstones", n)
	}
}

// TestPendingErasureOfALiveThreadIdIsNotRetried: a thread id that is live again may
// share its blob prefix with the new workspace, so its tombstone is not listed.
func TestPendingErasureOfALiveThreadIdIsNotRetried(t *testing.T) {
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
	if _, err := store.Apply(ctx, "org_1", "user_1", "thr_r", file, nil, limits); err != nil {
		t.Fatalf("apply: %v", err)
	}
	if _, err := eraser.Erase(ctx, "org_1", "user_1", "thr_r"); err != nil {
		t.Fatalf("Erase: %v", err)
	}
	// The same thread id is used again: its header is live.
	if _, err := store.Apply(ctx, "org_1", "user_1", "thr_r", file, nil, limits); err != nil {
		t.Fatalf("re-apply: %v", err)
	}
	ageTombstones(t, pool, 5*time.Minute)

	pending, err := eraser.ListPendingBlobErasures(ctx, time.Now().Add(-time.Minute), sweepBatch)
	if err != nil {
		t.Fatalf("ListPendingBlobErasures: %v", err)
	}
	if len(pending) != 0 {
		t.Fatalf("pending = %v; a live thread id's tombstone must not be retried", pending)
	}
}

// TestPendingErasuresAreListedOnlyOnceOldEnoughAndInOrder: a tombstone younger than
// the cutoff is left for a later pass, and the older ones come oldest first.
func TestPendingErasuresAreListedOnlyOnceOldEnoughAndInOrder(t *testing.T) {
	pool := setupThreadDB(t)
	ctx := context.Background()
	eraser, err := NewEraser(pool)
	if err != nil {
		t.Fatalf("NewEraser: %v", err)
	}
	older := ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_older"}
	oldest := ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_oldest"}
	young := ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_young"}
	for _, key := range []ThreadKey{older, oldest, young} {
		if _, err := eraser.Erase(ctx, key.OrgID, key.OwnerID, key.ThreadID); err != nil {
			t.Fatalf("Erase %s: %v", key.ThreadID, err)
		}
	}
	for thread, age := range map[string]string{"thr_oldest": "3 hours", "thr_older": "2 hours", "thr_young": "10 seconds"} {
		if _, err := pool.Exec(ctx, `UPDATE thread_workspace_erasures SET requested_at = now() - $2::interval WHERE thread_id = $1`, thread, age); err != nil {
			t.Fatalf("age %s: %v", thread, err)
		}
	}

	pending, err := eraser.ListPendingBlobErasures(ctx, time.Now().Add(-time.Minute), sweepBatch)
	if err != nil {
		t.Fatalf("ListPendingBlobErasures: %v", err)
	}
	if want := []ThreadKey{oldest, older}; !slices.Equal(pending, want) {
		t.Fatalf("pending = %v, want %v (oldest first, the young one left)", pending, want)
	}
}

// TestCompleteIsIdempotentAndAnAttemptOnAGoneTombstoneCountsZero: closing a tombstone
// twice, or recording an attempt on one already closed, is not an error.
func TestCompleteIsIdempotentAndAnAttemptOnAGoneTombstoneCountsZero(t *testing.T) {
	pool := setupThreadDB(t)
	ctx := context.Background()
	eraser, err := NewEraser(pool)
	if err != nil {
		t.Fatalf("NewEraser: %v", err)
	}
	gone := ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_gone"}
	if err := eraser.CompleteBlobErasure(ctx, gone); err != nil {
		t.Fatalf("complete with no tombstone: %v", err)
	}
	if err := eraser.CompleteBlobErasure(ctx, gone); err != nil {
		t.Fatalf("complete twice: %v", err)
	}
	if n, err := eraser.RecordBlobErasureAttempt(ctx, gone); err != nil || n != 0 {
		t.Fatalf("attempt on a gone tombstone = (%d, %v), want (0, nil)", n, err)
	}
}

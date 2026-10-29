package rawstore

import (
	"errors"
	"testing"
	"time"
)

// T60: one budget, one sync. A second TryLock is refused while the first lock
// is held, and succeeds once it is released.
func TestBudgetLockIsExclusive(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)

	first, err := s.TryLock(ctx)
	if err != nil {
		t.Fatalf("first TryLock: %v", err)
	}
	if _, err := s.TryLock(ctx); !errors.Is(err, ErrLocked) {
		t.Fatalf("second TryLock while held: err = %v, want ErrLocked", err)
	}
	if err := first.Release(ctx); err != nil {
		t.Fatalf("Release: %v", err)
	}

	second, err := s.TryLock(ctx)
	if err != nil {
		t.Fatalf("TryLock after Release: %v", err)
	}
	if err := second.Release(ctx); err != nil {
		t.Fatalf("second Release: %v", err)
	}
}

// T61: the lock lives only as long as its connection. Terminating the holder
// frees the lock, so a process that dies cannot keep the budget closed.
func TestBudgetLockDiesWithItsConnection(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)

	holder, err := s.TryLock(ctx)
	if err != nil {
		t.Fatalf("TryLock: %v", err)
	}
	pid := int32(holder.conn.PgConn().PID())
	var terminated bool
	if err := pool.QueryRow(ctx, `SELECT pg_terminate_backend($1)`, pid).Scan(&terminated); err != nil || !terminated {
		t.Fatalf("pg_terminate_backend(%d) = %v, %v; want true", pid, terminated, err)
	}
	_ = holder.Release(ctx) // the connection is gone; the error is expected

	// Termination is asynchronous. Poll briefly, and fail only if the lock never frees.
	deadline := time.Now().Add(10 * time.Second)
	for {
		lock, err := s.TryLock(ctx)
		if err == nil {
			if err := lock.Release(ctx); err != nil {
				t.Fatalf("Release after the holder died: %v", err)
			}
			return
		}
		if !errors.Is(err, ErrLocked) {
			t.Fatalf("TryLock after the holder died: %v", err)
		}
		if time.Now().After(deadline) {
			t.Fatal("the budget lock outlived its terminated connection")
		}
		time.Sleep(50 * time.Millisecond)
	}
}

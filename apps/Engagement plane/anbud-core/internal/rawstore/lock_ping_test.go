package rawstore

import (
	"testing"
	"time"
)

// S3 (review PG-01): Ping is nil only while the lock's session is alive. When the
// holder's session is terminated, the session's locks go with it, so the ping
// must start to fail.
func TestBudgetLockPingFailsAfterTheHolderDies(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)

	holder, err := s.TryLock(ctx)
	if err != nil {
		t.Fatalf("TryLock: %v", err)
	}
	defer func() { _ = holder.Release(ctx) }() // the connection may be gone; the error is expected

	if err := holder.Ping(ctx); err != nil {
		t.Fatalf("Ping while the holder is alive: %v", err)
	}

	pid := int32(holder.conn.PgConn().PID())
	var terminated bool
	if err := pool.QueryRow(ctx, `SELECT pg_terminate_backend($1)`, pid).Scan(&terminated); err != nil || !terminated {
		t.Fatalf("pg_terminate_backend(%d) = %v, %v; want true", pid, terminated, err)
	}
	// Termination is asynchronous. Poll briefly; fail only if the ping keeps passing.
	deadline := time.Now().Add(10 * time.Second)
	for holder.Ping(ctx) == nil {
		if time.Now().After(deadline) {
			t.Fatal("Ping still returns nil after the holder's session was terminated")
		}
		time.Sleep(50 * time.Millisecond)
	}
}

// S3: a live session that no longer holds the lock fails the ping. SELECT 1 still
// answers on such a session, so only the pg_locks check can refuse it.
func TestBudgetLockPingFailsWhenTheLockIsNoLongerHeld(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)

	holder, err := s.TryLock(ctx)
	if err != nil {
		t.Fatalf("TryLock: %v", err)
	}
	defer func() { _ = holder.Release(ctx) }() // the lock is already gone; the error is expected

	if err := holder.Ping(ctx); err != nil {
		t.Fatalf("Ping while the lock is held: %v", err)
	}
	if _, err := holder.conn.Exec(ctx, `SELECT pg_advisory_unlock($1)`, budgetLockKey); err != nil {
		t.Fatalf("unlock on the holder's session: %v", err)
	}
	if err := holder.Ping(ctx); err == nil {
		t.Fatal("Ping returned nil on a live session that no longer holds the lock")
	}
}

package rawstore

import (
	"context"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"
)

// budgetLockKey is the session advisory lock that serialises every sync calling
// Doffin on one key. It spells "doffsync". The migration lock is a different key.
const budgetLockKey int64 = 0x646F666673796E63

// lockClassID and lockObjectID split budgetLockKey as pg_locks shows a bigint
// advisory key: the high 32 bits as classid, the low 32 bits as objid.
const (
	lockClassID  = budgetLockKey >> 32
	lockObjectID = budgetLockKey & 0xFFFFFFFF
)

// ErrLocked means another session holds the budget lock: one Doffin key, one sync.
var ErrLocked = errors.New("rawstore: budget lock held by another sync")

// ErrLockNotHeld means the budget lock is not held by the session it was taken on.
var ErrLockNotHeld = errors.New("rawstore: budget lock is not held")

// Lock is the budget lock. It owns a connection taken out of the pool, so the
// session lock lives exactly as long as that connection. The connection is never
// returned to the pool: a pooled connection would hand the lock to its next user.
type Lock struct {
	conn *pgx.Conn
}

// TryLock takes the budget lock without waiting. It returns ErrLocked when another
// session holds it, and then leaves no connection behind.
func (s *Store) TryLock(ctx context.Context) (*Lock, error) {
	conn, err := s.pool.Acquire(ctx)
	if err != nil {
		return nil, fmt.Errorf("rawstore: acquire connection for budget lock: %w", err)
	}
	var held bool
	if err := conn.QueryRow(ctx, `SELECT pg_try_advisory_lock($1)`, budgetLockKey).Scan(&held); err != nil {
		conn.Release()
		return nil, fmt.Errorf("rawstore: try budget lock: %w", err)
	}
	if !held {
		conn.Release()
		return nil, ErrLocked
	}
	return &Lock{conn: conn.Hijack()}, nil
}

// Release unlocks, then closes the lock's connection. The close happens even when
// the unlock fails, so the session lock ends with the session.
func (l *Lock) Release(ctx context.Context) error {
	if l == nil || l.conn == nil {
		return nil
	}
	conn := l.conn
	l.conn = nil

	var unlocked bool
	unlockErr := conn.QueryRow(ctx, `SELECT pg_advisory_unlock($1)`, budgetLockKey).Scan(&unlocked)
	closeErr := conn.Close(ctx)
	switch {
	case unlockErr != nil:
		return errors.Join(fmt.Errorf("rawstore: unlock budget: %w", unlockErr), closeErr)
	case !unlocked:
		return errors.Join(errors.New("rawstore: budget lock was not held at release"), closeErr)
	case closeErr != nil:
		return fmt.Errorf("rawstore: close lock connection: %w", closeErr)
	}
	return nil
}

// selectLockHeld reports whether the current session holds the budget lock.
const selectLockHeld = `
SELECT EXISTS (
    SELECT 1 FROM pg_locks
    WHERE locktype = 'advisory' AND pid = pg_backend_pid() AND granted
      AND classid::bigint = $1 AND objid::bigint = $2 AND objsubid = 1)`

// Ping reports nil only while the lock's session is alive and still holds the
// budget lock. A terminated session has released the lock, and a session that
// unlocked it no longer excludes another sync. Either way the caller must stop.
func (l *Lock) Ping(ctx context.Context) error {
	if l == nil || l.conn == nil {
		return ErrLockNotHeld
	}
	var one int
	if err := l.conn.QueryRow(ctx, `SELECT 1`).Scan(&one); err != nil {
		return fmt.Errorf("rawstore: budget lock session is not alive: %w", err)
	}
	var held bool
	if err := l.conn.QueryRow(ctx, selectLockHeld, lockClassID, lockObjectID).Scan(&held); err != nil {
		return fmt.Errorf("rawstore: check budget lock: %w", err)
	}
	if !held {
		return ErrLockNotHeld
	}
	return nil
}

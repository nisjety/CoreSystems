package store

import (
	"context"
	"fmt"
	"hash/fnv"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Locks provides Postgres session-level advisory locks so multiple finspo-api
// replicas do not run the same per-source sync concurrently.
type Locks struct {
	pool *pgxpool.Pool
}

// WithSourceLock acquires a session-scoped advisory lock keyed on sourceID,
// runs fn, then releases the lock. If the lock is already held by another
// connection (another replica), fn is NOT run and acquired=false is returned
// with a nil error — the caller should treat that as "someone else has it".
//
// The lock is bound to a single dedicated connection checked out of the pool
// for the whole duration, because advisory locks are connection-scoped.
func (l *Locks) WithSourceLock(ctx context.Context, sourceID uuid.UUID, fn func(context.Context) error) (acquired bool, err error) {
	conn, err := l.pool.Acquire(ctx)
	if err != nil {
		return false, fmt.Errorf("acquire conn for advisory lock: %w", err)
	}
	defer conn.Release()

	key := advisoryKey(sourceID)

	if err := conn.QueryRow(ctx, `SELECT pg_try_advisory_lock($1)`, key).Scan(&acquired); err != nil {
		return false, fmt.Errorf("pg_try_advisory_lock: %w", err)
	}
	if !acquired {
		return false, nil
	}
	defer func() {
		// Best-effort unlock; if the connection died the lock is released
		// automatically when the session ends.
		_, _ = conn.Exec(context.WithoutCancel(ctx), `SELECT pg_advisory_unlock($1)`, key)
	}()

	return true, fn(ctx)
}

// advisoryKey derives a stable bigint key from a source UUID via FNV-1a.
// Collisions are harmless here — a false collision only serializes two
// unrelated sources occasionally, which is acceptable.
func advisoryKey(id uuid.UUID) int64 {
	h := fnv.New64a()
	_, _ = h.Write(id[:])
	return int64(h.Sum64())
}

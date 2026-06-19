package database

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

type DB struct {
	Pool *pgxpool.Pool
}

func Connect(ctx context.Context, dsn string) (*DB, error) {
	cfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		return nil, fmt.Errorf("parse pg config: %w", err)
	}

	// Pool tuned for 0.25 CPU / 256MB resource limit per container
	// Each pgx conn uses ~5MB; 5 conns = 25MB overhead, leaves headroom
	cfg.MaxConns = 5
	cfg.MinConns = 2
	cfg.MaxConnLifetime = 30 * time.Minute // Recycle connections more aggressively
	cfg.MaxConnIdleTime = 5 * time.Minute  // Free idle connections faster
	cfg.HealthCheckPeriod = 30 * time.Second // Detect failures faster

	pool, err := pgxpool.NewWithConfig(ctx, cfg)
	if err != nil {
		return nil, fmt.Errorf("create pg pool: %w", err)
	}
	if err := pool.Ping(ctx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("ping pg: %w", err)
	}

	return &DB{Pool: pool}, nil
}

func (db *DB) Close() {
	if db != nil && db.Pool != nil {
		db.Pool.Close()
	}
}

func (db *DB) Health(ctx context.Context) error {
	ctx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	return db.Pool.Ping(ctx)
}

// WithOrgScope runs fn inside a transaction that has the request-scoped GUC
// `app.current_org` set to orgID via SET LOCAL. This is the application half of
// the Row-Level Security backstop introduced (gated) in migration 008: the RLS
// policies key on current_setting('app.current_org'), and SET LOCAL ensures the
// value is scoped to this transaction only (it is reset automatically on COMMIT
// or ROLLBACK, so it cannot leak across pooled connections).
//
// SET LOCAL cannot be parameterized, so orgID is passed through quote_literal
// via set_config(..., true) — set_config's `is_local = true` argument is the
// transaction-scoped equivalent of SET LOCAL and safely accepts orgID as a bind
// parameter, eliminating any injection surface.
//
// NOTE: This is delivered for the gated RLS migration and is a no-op for
// correctness while RLS is disabled (the GUC simply goes unread). Wire repo
// mutations through it only once migration 008 has been validated and enabled
// on a real database (see migrations/008_rls_tenant_isolation.up.sql).
func (db *DB) WithOrgScope(ctx context.Context, orgID string, fn func(tx pgx.Tx) error) error {
	tx, err := db.Pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin org-scoped tx: %w", err)
	}
	// Roll back on any path that doesn't reach an explicit Commit. A no-op
	// after a successful Commit.
	defer func() { _ = tx.Rollback(ctx) }()

	// set_config('app.current_org', $1, true) == SET LOCAL app.current_org = $1,
	// but parameterized (SET LOCAL itself cannot bind parameters).
	if _, err := tx.Exec(ctx, "SELECT set_config('app.current_org', $1, true)", orgID); err != nil {
		return fmt.Errorf("set org scope: %w", err)
	}

	if err := fn(tx); err != nil {
		return err
	}

	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit org-scoped tx: %w", err)
	}
	return nil
}

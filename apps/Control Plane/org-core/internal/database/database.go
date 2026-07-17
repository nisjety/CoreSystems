package database

import (
	"context"
	"fmt"
	"os"
	"strconv"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

type DB struct {
	Pool *pgxpool.Pool
}

// poolInt32Env reads a positive int32 pool bound from the environment, falling
// back to def. org-core is the hottest shared read path (org/entitlement
// lookups for every plane); this lets a higher-resource deployment raise the
// ceiling without a rebuild while the default stays safe for a 256MB container.
func poolInt32Env(key string, def int32) int32 {
	if v := os.Getenv(key); v != "" {
		if n, err := strconv.Atoi(v); err == nil && n > 0 {
			return int32(n)
		}
	}
	return def
}

func Connect(ctx context.Context, dsn string) (*DB, error) {
	cfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		return nil, fmt.Errorf("parse pg config: %w", err)
	}

	// Pool tuned for 0.25 CPU / 256MB resource limit per container
	// Each pgx conn uses ~5MB; 5 conns = 25MB overhead, leaves headroom.
	// Env-tunable (DB_MAX_CONNS/DB_MIN_CONNS) for higher-resource deployments.
	cfg.MaxConns = poolInt32Env("DB_MAX_CONNS", 5)
	cfg.MinConns = poolInt32Env("DB_MIN_CONNS", 2)
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

// rlsRuntimeRole is the NOLOGIN, NOSUPERUSER, NOBYPASSRLS role created by
// migration 009. WithOrgScope drops to it (SET LOCAL ROLE) inside each scoped
// transaction so the RLS policies actually apply — the pooled connection
// authenticates as a superuser/owner, which would otherwise bypass RLS. It is a
// fixed identifier (never user input), so it is safe to interpolate into SQL.
const rlsRuntimeRole = "org_core_app"

// WithOrgScope runs fn inside a transaction that (a) sets the request-scoped GUC
// `app.current_org` to orgID and (b) drops to the non-superuser runtime role, so
// the Row-Level Security policies from migration 009 enforce a hard DB-level
// tenant filter for the duration of the call.
//
// Both settings are transaction-local (SET LOCAL / set_config(..., is_local=true)
// / SET LOCAL ROLE), so they reset automatically on COMMIT or ROLLBACK and can
// never leak across pooled connections — after the call the connection is back
// to its original (superuser) identity for the next caller.
//
// SET LOCAL cannot bind parameters, so orgID flows through set_config(..., true)
// — the transaction-scoped equivalent of SET LOCAL that safely accepts orgID as
// a bind parameter, eliminating any injection surface. The GUC is set BEFORE the
// role is dropped so the superuser connection establishes it without privilege
// concerns; the value persists through the role change for the rest of the tx.
//
// Use this for every org-scoped single-tenant request path. Genuinely cross-org
// or multi-org paths (admin list-all, "orgs for this user", lookup by secondary
// key, GDPR erasure procs) must NOT use it — they run as the superuser
// connection and intentionally see across tenants, gated by the app authz layer.
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

	// Drop superuser for this transaction so RLS policies apply. rlsRuntimeRole
	// is a trusted constant, not user input.
	if _, err := tx.Exec(ctx, "SET LOCAL ROLE "+rlsRuntimeRole); err != nil {
		return fmt.Errorf("set rls runtime role: %w", err)
	}

	if err := fn(tx); err != nil {
		return err
	}

	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit org-scoped tx: %w", err)
	}
	return nil
}

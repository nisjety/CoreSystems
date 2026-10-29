package database

import (
	"context"
	"fmt"
	"os"
	"strconv"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

type DB struct {
	Pool *pgxpool.Pool
}

// poolInt32Env reads a positive int32 pool bound from the environment, falling
// back to def. Lets deployments with more than the default 0.25 CPU / 256MB
// headroom raise connection limits without a rebuild; the default stays safe
// for the constrained container.
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
	cfg.MaxConnLifetime = 30 * time.Minute   // Recycle connections more aggressively
	cfg.MaxConnIdleTime = 5 * time.Minute    // Free idle connections faster
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

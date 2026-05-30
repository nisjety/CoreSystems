package database

import (
	"context"
	"fmt"
	"time"

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

// Package database wires insight-core's optional Postgres pool. Phase-1 A ran
// insight-core registry-only with an in-memory metric repo (replicas: 1); W3
// (PR-3) adds durable metric persistence so real per-org events survive restarts
// and can back briefs. The pool is OPTIONAL: when DATABASE_URL is unset the
// service falls back to the in-memory repo (unchanged Phase-1 behaviour).
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

func Connect(ctx context.Context, databaseURL string) (*DB, error) {
	cfg, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		return nil, fmt.Errorf("parse database url: %w", err)
	}
	cfg.MaxConns = 10
	cfg.MinConns = 1
	cfg.MaxConnLifetime = time.Hour
	cfg.HealthCheckPeriod = 30 * time.Second

	pool, err := pgxpool.NewWithConfig(ctx, cfg)
	if err != nil {
		return nil, fmt.Errorf("create pool: %w", err)
	}
	if err := pool.Ping(ctx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("ping postgres: %w", err)
	}
	return &DB{Pool: pool}, nil
}

func (db *DB) Close() {
	if db == nil || db.Pool == nil {
		return
	}
	db.Pool.Close()
}

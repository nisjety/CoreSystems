package database

import (
	"context"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/config"
	"github.com/jackc/pgx/v5/pgxpool"
)

// DB wraps the database connection pool
type DB struct {
	Pool *pgxpool.Pool
}

// Connect creates a new database connection pool
func Connect(ctx context.Context, cfg *config.DatabaseConfig) (*DB, error) {
	poolConfig, err := pgxpool.ParseConfig(cfg.GetDatabaseDSN())
	if err != nil {
		return nil, fmt.Errorf("failed to parse database config: %w", err)
	}

	// Pool tuned for 0.25 CPU / 256MB resource limit per container
	// MaxConns/MinConns come from DatabaseConfig (set by env vars, default 5/2)
	poolConfig.MaxConns = int32(cfg.MaxConnections)
	poolConfig.MinConns = int32(cfg.MinConnections)
	poolConfig.MaxConnLifetime = 30 * time.Minute   // Recycle more aggressively
	poolConfig.MaxConnIdleTime = 5 * time.Minute    // Free idle connections faster
	poolConfig.HealthCheckPeriod = 30 * time.Second // Detect failures faster

	// Create connection pool
	pool, err := pgxpool.NewWithConfig(ctx, poolConfig)
	if err != nil {
		return nil, fmt.Errorf("failed to create connection pool: %w", err)
	}

	// Verify connection
	if err := pool.Ping(ctx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("failed to ping database: %w", err)
	}

	return &DB{Pool: pool}, nil
}

// Close closes the database connection pool
func (db *DB) Close() {
	if db.Pool != nil {
		db.Pool.Close()
	}
}

// Ping verifies the database connection
func (db *DB) Ping(ctx context.Context) error {
	return db.Pool.Ping(ctx)
}

// Health returns the health status of the database
func (db *DB) Health(ctx context.Context) error {
	ctx, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()

	return db.Ping(ctx)
}

// RunMigrations applies all pending *.up.sql files from migrationsDir in lexicographic order.
// Tracks applied migrations in a schema_migrations table — idempotent across restarts.
func (db *DB) RunMigrations(ctx context.Context, migrationsDir string) error {
	// Ensure tracking table exists
	_, err := db.Pool.Exec(ctx, `
		CREATE TABLE IF NOT EXISTS schema_migrations (
			version    VARCHAR(255) PRIMARY KEY,
			applied_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
		)
	`)
	if err != nil {
		return fmt.Errorf("failed to create schema_migrations table: %w", err)
	}

	// Bootstrap: if schema_migrations was just created empty but the database
	// already has the users table, the first migration ran before this runner existed.
	// Mark it as applied so we don't attempt to re-run it.
	var migrationsEmpty bool
	err = db.Pool.QueryRow(ctx,
		"SELECT NOT EXISTS(SELECT 1 FROM schema_migrations)",
	).Scan(&migrationsEmpty)
	if err == nil && migrationsEmpty {
		var usersExist bool
		_ = db.Pool.QueryRow(ctx,
			"SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_schema='public' AND table_name='users')",
		).Scan(&usersExist)
		if usersExist {
			log.Printf("bootstrapping: users table already exists, marking 001_init as applied")
			_, _ = db.Pool.Exec(ctx,
				"INSERT INTO schema_migrations (version) VALUES ('001_init') ON CONFLICT DO NOTHING",
			)
		}
	}

	entries, err := os.ReadDir(migrationsDir)
	if err != nil {
		return fmt.Errorf("failed to read migrations directory %q: %w", migrationsDir, err)
	}

	// Collect and sort .up.sql files
	var files []string
	for _, e := range entries {
		if !e.IsDir() && strings.HasSuffix(e.Name(), ".up.sql") {
			files = append(files, e.Name())
		}
	}
	sort.Strings(files)

	for _, name := range files {
		version := strings.TrimSuffix(name, ".up.sql")

		var applied bool
		err := db.Pool.QueryRow(ctx,
			"SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = $1)",
			version,
		).Scan(&applied)
		if err != nil {
			return fmt.Errorf("failed to check migration %s: %w", version, err)
		}
		if applied {
			log.Printf("migration %s already applied, skipping", version)
			continue
		}

		content, err := os.ReadFile(filepath.Join(migrationsDir, name))
		if err != nil {
			return fmt.Errorf("failed to read migration %s: %w", version, err)
		}

		if _, err = db.Pool.Exec(ctx, string(content)); err != nil {
			return fmt.Errorf("failed to apply migration %s: %w", version, err)
		}

		if _, err = db.Pool.Exec(ctx,
			"INSERT INTO schema_migrations (version) VALUES ($1)",
			version,
		); err != nil {
			return fmt.Errorf("failed to record migration %s: %w", version, err)
		}

		log.Printf("✅ Applied migration: %s", version)
	}

	return nil
}

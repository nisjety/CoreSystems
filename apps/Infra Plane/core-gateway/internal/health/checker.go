package health

import (
	"context"
	"database/sql"
	"fmt"
	"time"

	"github.com/coresystem/integration-gateway/internal/cache"
	_ "github.com/lib/pq"
)

// DatabaseChecker checks database connectivity and health
type DatabaseChecker struct {
	postgresURL string
	redisCache  *cache.RedisCache
}

// Config holds database checker configuration
type Config struct {
	PostgresURL string
	RedisCache  *cache.RedisCache
}

// NewDatabaseChecker creates a new database health checker
func NewDatabaseChecker(cfg Config) *DatabaseChecker {
	return &DatabaseChecker{
		postgresURL: cfg.PostgresURL,
		redisCache:  cfg.RedisCache,
	}
}

// PostgreSQLHealth represents PostgreSQL health status
type PostgreSQLHealth struct {
	Status         string        `json:"status"`
	Version        string        `json:"version,omitempty"`
	Connections    int           `json:"connections"`
	MaxConnections int           `json:"max_connections"`
	DatabaseSize   string        `json:"database_size,omitempty"`
	ResponseTime   time.Duration `json:"response_time_ms"`
	Error          string        `json:"error,omitempty"`
}

// RedisHealth represents Redis health status
type RedisHealth struct {
	Status           string        `json:"status"`
	Version          string        `json:"version,omitempty"`
	ConnectedClients int           `json:"connected_clients"`
	UsedMemory       uint64        `json:"used_memory"`
	ResponseTime     time.Duration `json:"response_time_ms"`
	Error            string        `json:"error,omitempty"`
}

// CheckPostgreSQL checks PostgreSQL database health
func (d *DatabaseChecker) CheckPostgreSQL(ctx context.Context) PostgreSQLHealth {
	start := time.Now()
	health := PostgreSQLHealth{
		Status: "unhealthy",
	}

	// Connect to database
	db, err := sql.Open("postgres", d.postgresURL)
	if err != nil {
		health.Error = fmt.Sprintf("failed to open connection: %v", err)
		health.ResponseTime = time.Since(start)
		return health
	}
	defer db.Close()

	// Set connection timeout
	db.SetConnMaxLifetime(10 * time.Second)
	db.SetMaxOpenConns(5)

	// Ping database
	if err := db.PingContext(ctx); err != nil {
		health.Error = fmt.Sprintf("ping failed: %v", err)
		health.ResponseTime = time.Since(start)
		return health
	}

	// Get version
	var version string
	err = db.QueryRowContext(ctx, "SELECT version()").Scan(&version)
	if err != nil {
		health.Error = fmt.Sprintf("failed to get version: %v", err)
		health.ResponseTime = time.Since(start)
		return health
	}
	health.Version = version

	// Get connection stats
	var connections, maxConnections int
	err = db.QueryRowContext(ctx, `
		SELECT 
			(SELECT count(*) FROM pg_stat_activity),
			(SELECT setting::int FROM pg_settings WHERE name = 'max_connections')
	`).Scan(&connections, &maxConnections)
	if err != nil {
		health.Error = fmt.Sprintf("failed to get connection stats: %v", err)
		health.ResponseTime = time.Since(start)
		return health
	}
	health.Connections = connections
	health.MaxConnections = maxConnections

	// Get database size
	var dbSize string
	err = db.QueryRowContext(ctx, `
		SELECT pg_size_pretty(pg_database_size(current_database()))
	`).Scan(&dbSize)
	if err == nil {
		health.DatabaseSize = dbSize
	}

	health.Status = "healthy"
	health.ResponseTime = time.Since(start)
	return health
}

// CheckRedis checks Redis cache health
func (d *DatabaseChecker) CheckRedis(ctx context.Context) RedisHealth {
	start := time.Now()
	health := RedisHealth{
		Status: "unhealthy",
	}

	if d.redisCache == nil {
		health.Error = "Redis not configured"
		health.ResponseTime = time.Since(start)
		return health
	}

	// Ping Redis
	if err := d.redisCache.Ping(); err != nil {
		health.Error = fmt.Sprintf("ping failed: %v", err)
		health.ResponseTime = time.Since(start)
		return health
	}

	// Get Redis info (if we add this method to cache.RedisCache)
	stats := d.redisCache.PoolStats()
	health.ConnectedClients = int(stats.TotalConns - stats.IdleConns)

	health.Status = "healthy"
	health.ResponseTime = time.Since(start)
	return health
}

// CheckAll checks all databases
func (d *DatabaseChecker) CheckAll(ctx context.Context) map[string]interface{} {
	return map[string]interface{}{
		"postgres": d.CheckPostgreSQL(ctx),
		"redis":    d.CheckRedis(ctx),
	}
}

// ServiceHealth represents overall service health
type ServiceHealth struct {
	Status    string                 `json:"status"`
	Timestamp time.Time              `json:"timestamp"`
	Version   string                 `json:"version"`
	Uptime    time.Duration          `json:"uptime_seconds"`
	Database  map[string]interface{} `json:"databases"`
}

// HealthChecker performs comprehensive health checks
type HealthChecker struct {
	dbChecker *DatabaseChecker
	startTime time.Time
	version   string
}

// NewHealthChecker creates a new health checker
func NewHealthChecker(dbChecker *DatabaseChecker, version string) *HealthChecker {
	return &HealthChecker{
		dbChecker: dbChecker,
		startTime: time.Now(),
		version:   version,
	}
}

// Check performs a comprehensive health check
func (h *HealthChecker) Check(ctx context.Context) ServiceHealth {
	health := ServiceHealth{
		Status:    "healthy",
		Timestamp: time.Now(),
		Version:   h.version,
		Uptime:    time.Since(h.startTime),
		Database:  h.dbChecker.CheckAll(ctx),
	}

	// Check if any database is unhealthy
	if pg, ok := health.Database["postgres"].(PostgreSQLHealth); ok {
		if pg.Status != "healthy" {
			health.Status = "degraded"
		}
	}

	if redis, ok := health.Database["redis"].(RedisHealth); ok {
		if redis.Status != "healthy" && redis.Error != "Redis not configured" {
			health.Status = "degraded"
		}
	}

	return health
}

// Liveness returns a simple liveness check
func (h *HealthChecker) Liveness() bool {
	return true
}

// Readiness returns readiness status (checks critical dependencies)
func (h *HealthChecker) Readiness(ctx context.Context) bool {
	// Check PostgreSQL (critical)
	pgHealth := h.dbChecker.CheckPostgreSQL(ctx)
	if pgHealth.Status != "healthy" {
		return false
	}

	// Redis is optional, don't fail readiness if it's down
	return true
}

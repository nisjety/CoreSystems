package cache

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"time"
)

// QueryCache provides caching for database query results
type QueryCache struct {
	redis      *RedisCache
	defaultTTL time.Duration
	prefix     string
}

// QueryCacheConfig holds query cache configuration
type QueryCacheConfig struct {
	Redis      *RedisCache
	DefaultTTL time.Duration // Default time-to-live for cached queries
	Prefix     string        // Key prefix for namespacing
}

// NewQueryCache creates a new query cache
func NewQueryCache(cfg QueryCacheConfig) *QueryCache {
	if cfg.DefaultTTL == 0 {
		cfg.DefaultTTL = 5 * time.Minute
	}
	if cfg.Prefix == "" {
		cfg.Prefix = "query"
	}

	return &QueryCache{
		redis:      cfg.Redis,
		defaultTTL: cfg.DefaultTTL,
		prefix:     cfg.Prefix,
	}
}

// Get retrieves a cached query result
func (qc *QueryCache) Get(ctx context.Context, query string, dest interface{}) (bool, error) {
	if qc.redis == nil {
		return false, fmt.Errorf("redis cache not configured")
	}

	key := qc.generateKey(query)
	err := qc.redis.Get(key, dest)
	if err != nil {
		// Cache miss or error
		return false, nil
	}

	return true, nil
}

// Set stores a query result in cache
func (qc *QueryCache) Set(ctx context.Context, query string, data interface{}, ttl ...time.Duration) error {
	if qc.redis == nil {
		return fmt.Errorf("redis cache not configured")
	}

	// Determine TTL
	cacheTTL := qc.defaultTTL
	if len(ttl) > 0 && ttl[0] > 0 {
		cacheTTL = ttl[0]
	}

	key := qc.generateKey(query)
	return qc.redis.Set(key, data, cacheTTL)
}

// Invalidate removes a cached query result
func (qc *QueryCache) Invalidate(ctx context.Context, query string) error {
	if qc.redis == nil {
		return fmt.Errorf("redis cache not configured")
	}

	key := qc.generateKey(query)
	return qc.redis.Delete(key)
}

// InvalidatePattern removes all cached queries matching a pattern
func (qc *QueryCache) InvalidatePattern(ctx context.Context, pattern string) error {
	if qc.redis == nil {
		return fmt.Errorf("redis cache not configured")
	}

	// This would require Redis SCAN command - simplified version
	// In production, implement proper pattern matching with SCAN
	return fmt.Errorf("pattern invalidation not implemented")
}

// generateKey creates a cache key from a query string
func (qc *QueryCache) generateKey(query string) string {
	// Use SHA256 hash of query as key (deterministic and collision-resistant)
	hash := sha256.Sum256([]byte(query))
	hashStr := hex.EncodeToString(hash[:])
	return fmt.Sprintf("%s:%s", qc.prefix, hashStr)
}

// GetStats returns cache statistics
func (qc *QueryCache) GetStats() map[string]interface{} {
	if qc.redis == nil {
		return map[string]interface{}{
			"enabled": false,
		}
	}

	stats := qc.redis.PoolStats()
	return map[string]interface{}{
		"enabled":     true,
		"default_ttl": qc.defaultTTL.String(),
		"prefix":      qc.prefix,
		"pool_stats": map[string]interface{}{
			"total_conns": stats.TotalConns,
			"idle_conns":  stats.IdleConns,
			"hits":        stats.Hits,
			"misses":      stats.Misses,
		},
	}
}

// CacheWrapper wraps a query function with caching
func (qc *QueryCache) CacheWrapper(ctx context.Context, query string, ttl time.Duration, fn func() (interface{}, error)) (interface{}, error) {
	// Try to get from cache
	var result interface{}
	hit, err := qc.Get(ctx, query, &result)
	if err != nil {
		// Log error but continue to execute query
		fmt.Printf("Cache get error: %v\n", err)
	}

	if hit && result != nil {
		return result, nil
	}

	// Cache miss - execute query
	result, err = fn()
	if err != nil {
		return nil, err
	}

	// Store in cache (async to not block response)
	go func() {
		if err := qc.Set(context.Background(), query, result, ttl); err != nil {
			fmt.Printf("Cache set error: %v\n", err)
		}
	}()

	return result, nil
}

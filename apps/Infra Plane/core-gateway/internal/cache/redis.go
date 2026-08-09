package cache

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"fmt"
	"time"

	"github.com/coresystem/integration-gateway/internal/config"
	"github.com/redis/go-redis/v9"
)

// RedisCache wraps go-redis with encryption and connection pooling
type RedisCache struct {
	client *redis.Client
	ctx    context.Context
}

// NewRedisCache creates a new Redis cache with TLS encryption
func NewRedisCache(cfg config.RedisConfig) (*RedisCache, error) {
	opts := &redis.Options{
		Addr:         cfg.GetRedisAddr(),
		Password:     cfg.Password,
		DB:           cfg.DB,
		PoolSize:     cfg.PoolSize,
		DialTimeout:  cfg.Timeout,
		ReadTimeout:  cfg.Timeout,
		WriteTimeout: cfg.Timeout,
	}

	// Configure TLS if enabled
	if cfg.TLS.Enabled {
		opts.TLSConfig = &tls.Config{
			MinVersion:         cfg.TLS.MinVersion,
			InsecureSkipVerify: cfg.TLS.InsecureSkipVerify,
		}
	}

	client := redis.NewClient(opts)
	ctx := context.Background()

	// Test connection
	if err := client.Ping(ctx).Err(); err != nil {
		return nil, fmt.Errorf("failed to connect to Redis: %w", err)
	}

	return &RedisCache{
		client: client,
		ctx:    ctx,
	}, nil
}

// Set stores a value with expiration
func (r *RedisCache) Set(key string, value interface{}, expiration time.Duration) error {
	data, err := json.Marshal(value)
	if err != nil {
		return fmt.Errorf("failed to marshal value: %w", err)
	}

	return r.client.Set(r.ctx, key, data, expiration).Err()
}

// Get retrieves a value
func (r *RedisCache) Get(key string, dest interface{}) error {
	data, err := r.client.Get(r.ctx, key).Bytes()
	if err != nil {
		if err == redis.Nil {
			return fmt.Errorf("key not found: %s", key)
		}
		return fmt.Errorf("failed to get value: %w", err)
	}

	if err := json.Unmarshal(data, dest); err != nil {
		return fmt.Errorf("failed to unmarshal value: %w", err)
	}

	return nil
}

// Delete removes a key
func (r *RedisCache) Delete(key string) error {
	return r.client.Del(r.ctx, key).Err()
}

// Exists checks if a key exists
func (r *RedisCache) Exists(key string) (bool, error) {
	result, err := r.client.Exists(r.ctx, key).Result()
	return result > 0, err
}

// Expire sets expiration on a key
func (r *RedisCache) Expire(key string, expiration time.Duration) error {
	return r.client.Expire(r.ctx, key, expiration).Err()
}

// Increment atomically increments a counter
func (r *RedisCache) Increment(key string) (int64, error) {
	return r.client.Incr(r.ctx, key).Result()
}

// IncrementBy atomically increments a counter by value
func (r *RedisCache) IncrementBy(key string, value int64) (int64, error) {
	return r.client.IncrBy(r.ctx, key, value).Result()
}

// SetNX sets a key only if it doesn't exist (distributed lock)
func (r *RedisCache) SetNX(key string, value interface{}, expiration time.Duration) (bool, error) {
	data, err := json.Marshal(value)
	if err != nil {
		return false, fmt.Errorf("failed to marshal value: %w", err)
	}

	return r.client.SetNX(r.ctx, key, data, expiration).Result()
}

// GetTTL gets the remaining TTL of a key
func (r *RedisCache) GetTTL(key string) (time.Duration, error) {
	return r.client.TTL(r.ctx, key).Result()
}

// Keys returns all keys matching pattern
func (r *RedisCache) Keys(pattern string) ([]string, error) {
	return r.client.Keys(r.ctx, pattern).Result()
}

// FlushDB clears the current database
func (r *RedisCache) FlushDB() error {
	return r.client.FlushDB(r.ctx).Err()
}

// Close closes the Redis connection
func (r *RedisCache) Close() error {
	return r.client.Close()
}

// Ping tests the connection
func (r *RedisCache) Ping() error {
	return r.client.Ping(r.ctx).Err()
}

// PoolStats returns connection pool statistics
func (r *RedisCache) PoolStats() *redis.PoolStats {
	return r.client.PoolStats()
}

// Client returns the underlying Redis client
func (r *RedisCache) Client() *redis.Client {
	return r.client
}

// TokenCache provides methods for managing OAuth2 tokens
type TokenCache struct {
	cache  *RedisCache
	prefix string
}

// NewTokenCache creates a new token cache
func NewTokenCache(cache *RedisCache) *TokenCache {
	return &TokenCache{
		cache:  cache,
		prefix: "token:",
	}
}

// Token represents an OAuth2 token
type Token struct {
	AccessToken  string    `json:"access_token"`
	RefreshToken string    `json:"refresh_token,omitempty"`
	TokenType    string    `json:"token_type"`
	ExpiresAt    time.Time `json:"expires_at"`
	Scope        string    `json:"scope,omitempty"`
}

// SetToken stores an OAuth2 token
func (t *TokenCache) SetToken(service string, token Token) error {
	key := t.prefix + service
	expiration := time.Until(token.ExpiresAt)
	return t.cache.Set(key, token, expiration)
}

// GetToken retrieves an OAuth2 token
func (t *TokenCache) GetToken(service string) (*Token, error) {
	key := t.prefix + service
	var token Token
	if err := t.cache.Get(key, &token); err != nil {
		return nil, err
	}
	return &token, nil
}

// DeleteToken removes an OAuth2 token
func (t *TokenCache) DeleteToken(service string) error {
	key := t.prefix + service
	return t.cache.Delete(key)
}

// IsTokenValid checks if a token exists and is not expired
func (t *TokenCache) IsTokenValid(service string) (bool, error) {
	token, err := t.GetToken(service)
	if err != nil {
		return false, nil
	}
	return time.Now().Before(token.ExpiresAt), nil
}

package ai

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sync"
	"sync/atomic"
	"time"

	"github.com/redis/go-redis/v9"

	"github.com/triodelab/quarry/internal/config"
)

type cacheItem struct {
	value     string
	expiresAt time.Time
}

type ResponseCache struct {
	backend string

	mu          sync.RWMutex
	memoryStore map[string]cacheItem
	redisClient *redis.Client

	planTTL    time.Duration
	extractTTL time.Duration

	hits   int64
	misses int64
	sets   int64
	errors int64
}

func NewResponseCache(cfg *config.Config) *ResponseCache {
	c := &ResponseCache{
		backend:     "memory",
		memoryStore: make(map[string]cacheItem),
		planTTL:     24 * time.Hour,
		extractTTL:  1 * time.Hour,
	}

	if cfg == nil || cfg.RedisURL == "" {
		return c
	}

	if opts, err := redis.ParseURL(cfg.RedisURL); err == nil {
		client := redis.NewClient(opts)
		if pingErr := client.Ping(context.Background()).Err(); pingErr == nil {
			c.backend = "redis"
			c.redisClient = client
		}
	}

	return c
}

func (c *ResponseCache) Close() error {
	if c == nil || c.redisClient == nil {
		return nil
	}
	return c.redisClient.Close()
}

func (c *ResponseCache) GetPlan(ctx context.Context, url string) (string, bool) {
	if c == nil || url == "" {
		return "", false
	}
	key := fmt.Sprintf("aicore:plan:%s", shortHash(url))
	return c.get(ctx, key)
}

func (c *ResponseCache) SetPlan(ctx context.Context, url, value string) {
	if c == nil || url == "" || value == "" {
		return
	}
	key := fmt.Sprintf("aicore:plan:%s", shortHash(url))
	c.set(ctx, key, value, c.planTTL)
}

func (c *ResponseCache) GetExtract(ctx context.Context, html, schema string) (string, bool) {
	if c == nil || html == "" {
		return "", false
	}
	key := fmt.Sprintf("aicore:extract:%s:%s", shortHash(html), shortHash(schema))
	return c.get(ctx, key)
}

func (c *ResponseCache) SetExtract(ctx context.Context, html, schema, value string) {
	if c == nil || html == "" || value == "" {
		return
	}
	key := fmt.Sprintf("aicore:extract:%s:%s", shortHash(html), shortHash(schema))
	c.set(ctx, key, value, c.extractTTL)
}

func (c *ResponseCache) get(ctx context.Context, key string) (string, bool) {
	if c.backend == "redis" && c.redisClient != nil {
		val, err := c.redisClient.Get(ctx, key).Result()
		if err == nil {
			atomic.AddInt64(&c.hits, 1)
			return val, true
		}
		if err != redis.Nil {
			atomic.AddInt64(&c.errors, 1)
		}
		atomic.AddInt64(&c.misses, 1)
		return "", false
	}

	c.mu.RLock()
	item, ok := c.memoryStore[key]
	c.mu.RUnlock()
	if !ok {
		atomic.AddInt64(&c.misses, 1)
		return "", false
	}
	if time.Now().After(item.expiresAt) {
		c.mu.Lock()
		delete(c.memoryStore, key)
		c.mu.Unlock()
		atomic.AddInt64(&c.misses, 1)
		return "", false
	}

	atomic.AddInt64(&c.hits, 1)
	return item.value, true
}

func (c *ResponseCache) set(ctx context.Context, key, value string, ttl time.Duration) {
	if c.backend == "redis" && c.redisClient != nil {
		if err := c.redisClient.Set(ctx, key, value, ttl).Err(); err != nil {
			atomic.AddInt64(&c.errors, 1)
			return
		}
		atomic.AddInt64(&c.sets, 1)
		return
	}

	c.mu.Lock()
	c.memoryStore[key] = cacheItem{value: value, expiresAt: time.Now().Add(ttl)}
	c.mu.Unlock()
	atomic.AddInt64(&c.sets, 1)
}

func (c *ResponseCache) Snapshot() map[string]interface{} {
	if c == nil {
		return map[string]interface{}{"enabled": false}
	}
	hits := atomic.LoadInt64(&c.hits)
	misses := atomic.LoadInt64(&c.misses)
	sets := atomic.LoadInt64(&c.sets)
	errors := atomic.LoadInt64(&c.errors)

	totalLookups := hits + misses
	hitRatio := 0.0
	if totalLookups > 0 {
		hitRatio = float64(hits) / float64(totalLookups)
	}

	return map[string]interface{}{
		"enabled":                  true,
		"backend":                  c.backend,
		"hits":                     hits,
		"misses":                   misses,
		"sets":                     sets,
		"errors":                   errors,
		"hit_ratio":                hitRatio,
		"plan_ttl_seconds":         int(c.planTTL.Seconds()),
		"extract_ttl_seconds":      int(c.extractTTL.Seconds()),
		"estimated_ai_calls_saved": hits,
	}
}

func shortHash(input string) string {
	sum := sha256.Sum256([]byte(input))
	return hex.EncodeToString(sum[:8])
}

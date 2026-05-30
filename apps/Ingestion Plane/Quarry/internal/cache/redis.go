package cache

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/redis/go-redis/v9"
)

const redisCachePrefix = "quarry:cache:"

type RedisStore struct {
	client *redis.Client
	mu     sync.RWMutex
	stats  Stats
}

func NewRedisStore(redisURL string) (*RedisStore, error) {
	opts, err := redis.ParseURL(strings.TrimSpace(redisURL))
	if err != nil {
		return nil, fmt.Errorf("parse redis url: %w", err)
	}

	client := redis.NewClient(opts)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := client.Ping(ctx).Err(); err != nil {
		_ = client.Close()
		return nil, fmt.Errorf("ping redis: %w", err)
	}

	return &RedisStore{
		client: client,
		stats: Stats{
			CreatedAt:   time.Now(),
			LastUpdated: time.Now(),
		},
	}, nil
}

func (r *RedisStore) Get(ctx context.Context, key string) (interface{}, error) {
	payload, err := r.client.Get(ctx, redisCachePrefix+key).Bytes()
	if err != nil {
		r.recordMiss()
		return nil, ErrCacheMiss
	}

	var value interface{}
	if err := json.Unmarshal(payload, &value); err != nil {
		r.recordMiss()
		return nil, fmt.Errorf("decode redis cache payload: %w", err)
	}

	r.recordHit()
	return value, nil
}

func (r *RedisStore) Set(ctx context.Context, key string, value interface{}, ttl time.Duration) error {
	if ttl <= 0 {
		ttl = 24 * time.Hour
	}
	payload, err := json.Marshal(value)
	if err != nil {
		return fmt.Errorf("encode redis cache payload: %w", err)
	}
	if err := r.client.Set(ctx, redisCachePrefix+key, payload, ttl).Err(); err != nil {
		return err
	}

	r.mu.Lock()
	r.stats.LastUpdated = time.Now()
	r.mu.Unlock()
	return nil
}

func (r *RedisStore) Delete(ctx context.Context, key string) error {
	return r.client.Del(ctx, redisCachePrefix+key).Err()
}

func (r *RedisStore) Clear(ctx context.Context) error {
	var cursor uint64
	for {
		keys, nextCursor, err := r.client.Scan(ctx, cursor, redisCachePrefix+"*", 100).Result()
		if err != nil {
			return err
		}
		if len(keys) > 0 {
			if err := r.client.Del(ctx, keys...).Err(); err != nil {
				return err
			}
		}
		cursor = nextCursor
		if cursor == 0 {
			break
		}
	}
	return nil
}

func (r *RedisStore) Exists(ctx context.Context, key string) (bool, error) {
	count, err := r.client.Exists(ctx, redisCachePrefix+key).Result()
	if err != nil {
		return false, err
	}
	return count > 0, nil
}

func (r *RedisStore) GetStats(ctx context.Context) Stats {
	r.mu.RLock()
	defer r.mu.RUnlock()
	return r.stats
}

func (r *RedisStore) Close() error {
	if r == nil || r.client == nil {
		return nil
	}
	return r.client.Close()
}

func (r *RedisStore) recordHit() {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.stats.TotalRequests++
	r.stats.CacheHits++
	r.stats.LastUpdated = time.Now()
	r.updateHitRateLocked()
}

func (r *RedisStore) recordMiss() {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.stats.TotalRequests++
	r.stats.CacheMisses++
	r.stats.LastUpdated = time.Now()
	r.updateHitRateLocked()
}

func (r *RedisStore) updateHitRateLocked() {
	if r.stats.TotalRequests == 0 {
		r.stats.HitRate = 0
		return
	}
	r.stats.HitRate = float64(r.stats.CacheHits) / float64(r.stats.TotalRequests) * 100
}

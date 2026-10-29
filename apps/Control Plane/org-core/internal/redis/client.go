package redis

import (
	"context"
	"fmt"
	"log"
	"time"

	"github.com/redis/go-redis/v9"
)

// Config holds Redis connection configuration.
type Config struct {
	Host     string
	Port     string
	Password string
	DB       int
}

// Client wraps a Redis connection with convenience methods.
type Client struct {
	client *redis.Client
}

// NewClient creates a new Redis client and verifies connectivity.
func NewClient(cfg Config) (*Client, error) {
	addr := fmt.Sprintf("%s:%s", cfg.Host, cfg.Port)

	client := redis.NewClient(&redis.Options{
		Addr:         addr,
		Password:     cfg.Password,
		DB:           cfg.DB,
		DialTimeout:  5 * time.Second,
		ReadTimeout:  3 * time.Second,
		WriteTimeout: 3 * time.Second,
		PoolSize:     10,
		MinIdleConns: 5,
	})

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	if err := client.Ping(ctx).Err(); err != nil {
		return nil, fmt.Errorf("failed to connect to Redis at %s: %w", addr, err)
	}

	log.Printf("✅ Connected to Redis (org-core): %s", addr)

	return &Client{client: client}, nil
}

// Close closes the Redis connection.
func (c *Client) Close() error {
	if c.client != nil {
		if err := c.client.Close(); err != nil {
			return fmt.Errorf("failed to close Redis connection: %w", err)
		}
		log.Println("🔌 Redis connection closed (org-core)")
	}
	return nil
}

// Get retrieves a value from Redis by key. Returns error if key does not exist.
func (c *Client) Get(ctx context.Context, key string) (string, error) {
	val, err := c.client.Get(ctx, key).Result()
	if err == redis.Nil {
		return "", fmt.Errorf("key %s does not exist", key)
	} else if err != nil {
		return "", fmt.Errorf("failed to get key %s: %w", key, err)
	}
	return val, nil
}

// Set stores a value in Redis with an expiration duration.
func (c *Client) Set(ctx context.Context, key string, value interface{}, expiration time.Duration) error {
	if err := c.client.Set(ctx, key, value, expiration).Err(); err != nil {
		return fmt.Errorf("failed to set key %s: %w", key, err)
	}
	return nil
}

// Del deletes one or more keys from Redis.
func (c *Client) Del(ctx context.Context, keys ...string) error {
	if err := c.client.Del(ctx, keys...).Err(); err != nil {
		return fmt.Errorf("failed to delete keys: %w", err)
	}
	return nil
}

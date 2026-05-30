package redis

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	goredis "github.com/redis/go-redis/v9"
)

type Client struct {
	rdb *goredis.Client
}

func NewClient(addr, password string, db int) *Client {
	return &Client{
		rdb: goredis.NewClient(&goredis.Options{
			Addr:     addr,
			Password: password,
			DB:       db,
		}),
	}
}

func (c *Client) Close() {
	if c != nil && c.rdb != nil {
		c.rdb.Close()
	}
}

// --- Session State Cache ---

func (c *Client) CacheSessionState(ctx context.Context, sessionID string, state any, ttl time.Duration) error {
	if c == nil {
		return nil
	}
	data, err := json.Marshal(state)
	if err != nil {
		return fmt.Errorf("marshal session state: %w", err)
	}
	return c.rdb.Set(ctx, sessionKey(sessionID), data, ttl).Err()
}

func (c *Client) GetCachedSessionState(ctx context.Context, sessionID string, dest any) error {
	if c == nil {
		return fmt.Errorf("redis client is nil")
	}
	data, err := c.rdb.Get(ctx, sessionKey(sessionID)).Bytes()
	if err != nil {
		return err
	}
	return json.Unmarshal(data, dest)
}

func (c *Client) InvalidateSessionCache(ctx context.Context, sessionID string) error {
	if c == nil {
		return nil
	}
	return c.rdb.Del(ctx, sessionKey(sessionID)).Err()
}

// --- Event Cursor ---

func (c *Client) SetEventCursor(ctx context.Context, sessionID string, cursor int64) error {
	if c == nil {
		return nil
	}
	return c.rdb.Set(ctx, cursorKey(sessionID), cursor, 24*time.Hour).Err()
}

func (c *Client) GetEventCursor(ctx context.Context, sessionID string) (int64, error) {
	if c == nil {
		return 0, nil
	}
	return c.rdb.Get(ctx, cursorKey(sessionID)).Int64()
}

// --- Idempotency ---

func (c *Client) CheckAndSetIdempotency(ctx context.Context, key string, ttl time.Duration) (bool, error) {
	if c == nil {
		return false, nil
	}
	set, err := c.rdb.SetNX(ctx, idempotencyKey(key), 1, ttl).Result()
	if err != nil {
		return false, err
	}
	// SetNX returns true if the key was SET (first time), false if it already existed
	return !set, nil
}

// --- Control Session Cache (G34) ---

// CacheControlSession writes the aggregated Control Session snapshot for
// (userID, orgID) with a short TTL (30s is the recommended call site value
// per ADR 0002). The shape of `snap` is whatever the caller serializes —
// the redis layer is opaque to it.
func (c *Client) CacheControlSession(ctx context.Context, userID, orgID string, snap any, ttl time.Duration) error {
	if c == nil {
		return nil
	}
	data, err := json.Marshal(snap)
	if err != nil {
		return fmt.Errorf("marshal control session: %w", err)
	}
	return c.rdb.Set(ctx, controlSessionKey(userID, orgID), data, ttl).Err()
}

// GetCachedControlSession populates `dest` from the cached snapshot for
// (userID, orgID). Returns the underlying error so callers can distinguish
// `redis.Nil` (miss → fall through to upstream fan-out) from real failures.
func (c *Client) GetCachedControlSession(ctx context.Context, userID, orgID string, dest any) error {
	if c == nil {
		return goredis.Nil
	}
	data, err := c.rdb.Get(ctx, controlSessionKey(userID, orgID)).Bytes()
	if err != nil {
		return err
	}
	return json.Unmarshal(data, dest)
}

// InvalidateControlSession busts the entry for (userID, orgID). When orgID
// is the empty string, busts every cached snapshot for the user via
// `controlsession:<userID>:*` (used when the caller doesn't know which org
// the upstream event affected — e.g. notification-core forwarded the
// invalidation but didn't carry orgID).
func (c *Client) InvalidateControlSession(ctx context.Context, userID, orgID string) error {
	if c == nil {
		return nil
	}
	if orgID != "" {
		return c.rdb.Del(ctx, controlSessionKey(userID, orgID)).Err()
	}
	// Wildcard delete via SCAN. Scoped to one user so the iteration is bounded.
	iter := c.rdb.Scan(ctx, 0, controlSessionPrefix(userID)+"*", 100).Iterator()
	for iter.Next(ctx) {
		_ = c.rdb.Del(ctx, iter.Val()).Err()
	}
	return iter.Err()
}

// ErrCacheMiss exposes `redis.Nil` so callers can branch on it without
// importing the underlying client.
var ErrCacheMiss = goredis.Nil

// --- Key helpers ---

func sessionKey(id string) string      { return "session:" + id + ":state" }
func cursorKey(id string) string       { return "session:" + id + ":cursor" }
func idempotencyKey(key string) string { return "idempotency:" + key }

func controlSessionPrefix(userID string) string {
	return "controlsession:" + userID + ":"
}

func controlSessionKey(userID, orgID string) string {
	return controlSessionPrefix(userID) + orgID
}

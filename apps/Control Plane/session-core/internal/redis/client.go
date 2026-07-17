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

// --- Org -> Users reverse index (G34-followup-2) ---

// IndexOrgUser records `userID` in the org->users reverse-index set for
// `orgID` and (re)sets the set's TTL to `ttl`. It is maintained alongside
// every CacheControlSession write so an org-scoped upstream event (which
// carries no user_id) can resolve the exact set of users whose snapshots
// must be busted — see InvalidateOrgSessions — instead of waiting out the
// per-snapshot TTL.
//
// SADD + EXPIRE run in one pipeline round trip. Refreshing the TTL on every
// write makes the index self-healing: once writes stop, the set expires
// ~ttl later, so it never grows unbounded. `ttl` should be >= the snapshot
// TTL so the index reliably outlives the snapshots it points at.
//
// Best-effort by contract: callers treat a returned error as non-fatal (the
// snapshot TTL is the backstop) and must not fail the request on it. Never
// panics on a nil client.
func (c *Client) IndexOrgUser(ctx context.Context, orgID, userID string, ttl time.Duration) error {
	if c == nil || orgID == "" || userID == "" {
		return nil
	}
	key := orgUserIndexKey(orgID)
	pipe := c.rdb.Pipeline()
	pipe.SAdd(ctx, key, userID)
	pipe.Expire(ctx, key, ttl)
	if _, err := pipe.Exec(ctx); err != nil {
		return fmt.Errorf("index org user: %w", err)
	}
	return nil
}

// InvalidateOrgSessions busts the cached Control Session snapshots of every
// user recorded in the org->users reverse index for `orgID`, then clears the
// index set. It returns the number of users whose snapshot key was targeted.
//
// Used for org-scoped upstream events (e.g. organization.plan.changed) that
// carry no user_id: SMEMBERS resolves the affected users, and each snapshot
// for THIS org is deleted precisely (a user's snapshots for other orgs are
// left untouched). DEL is variadic and atomic in Redis. The index set is
// then dropped; members re-populate it on their next read-through cache
// write, and the set TTL is the backstop if this clear is ever lost.
//
// Best-effort by contract: a returned error is non-fatal for the caller (the
// snapshot TTL still bounds staleness). Never panics on a nil client or an
// unavailable Redis.
func (c *Client) InvalidateOrgSessions(ctx context.Context, orgID string) (int, error) {
	if c == nil || orgID == "" {
		return 0, nil
	}
	indexKey := orgUserIndexKey(orgID)
	userIDs, err := c.rdb.SMembers(ctx, indexKey).Result()
	if err != nil {
		return 0, fmt.Errorf("org index members: %w", err)
	}
	if len(userIDs) == 0 {
		return 0, nil
	}
	snapKeys := make([]string, 0, len(userIDs))
	for _, uid := range userIDs {
		snapKeys = append(snapKeys, controlSessionKey(uid, orgID))
	}
	// Snapshot deletes + index clear in one pipeline round trip.
	pipe := c.rdb.Pipeline()
	pipe.Del(ctx, snapKeys...)
	pipe.Del(ctx, indexKey)
	if _, err := pipe.Exec(ctx); err != nil {
		return 0, fmt.Errorf("invalidate org sessions: %w", err)
	}
	return len(userIDs), nil
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

// orgUserIndexKey names the org->users reverse-index set. Deliberately under
// a `control:sess:` namespace distinct from the `controlsession:` snapshot
// keyspace so a per-user wildcard snapshot SCAN can never match — or clobber
// — the index set.
func orgUserIndexKey(orgID string) string {
	return "control:sess:org:" + orgID
}

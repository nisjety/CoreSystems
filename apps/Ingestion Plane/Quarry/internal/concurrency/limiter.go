package concurrency

import (
	"context"
	"fmt"
	"time"

	"github.com/redis/go-redis/v9"
	"github.com/rs/zerolog/log"
)

// Limiter enforces per-team (or per-org) concurrency limits using a Redis
// sorted set. Each active job is a member scored by its start time. Before
// starting a new job the caller must Acquire a slot; on completion it must
// Release it.
//
// This is the same pattern Firecrawl uses (sorted-set per team) but
// implemented in Go and integrated with Quarry's Redis client.
type Limiter struct {
	rdb           *redis.Client
	keyPrefix     string
	defaultLimit  int
	slotTTL       time.Duration // Auto-expire stale slots.
}

// Config for the concurrency limiter.
type Config struct {
	RedisClient  *redis.Client
	KeyPrefix    string        // e.g. "quarry:concurrency:"
	DefaultLimit int           // Max concurrent jobs per team when no override.
	SlotTTL      time.Duration // Auto-expire stale slots (safety net).
}

// NewLimiter creates a new per-team concurrency limiter.
func NewLimiter(cfg Config) *Limiter {
	if cfg.KeyPrefix == "" {
		cfg.KeyPrefix = "quarry:concurrency:"
	}
	if cfg.DefaultLimit <= 0 {
		cfg.DefaultLimit = 10
	}
	if cfg.SlotTTL <= 0 {
		cfg.SlotTTL = 30 * time.Minute
	}
	return &Limiter{
		rdb:          cfg.RedisClient,
		keyPrefix:    cfg.KeyPrefix,
		defaultLimit: cfg.DefaultLimit,
		slotTTL:      cfg.SlotTTL,
	}
}

// Acquire attempts to claim a concurrency slot for the given team+jobID.
// Returns true if the slot was acquired, false if the team is at capacity.
// The limit can be overridden per-team; 0 means use defaultLimit.
func (l *Limiter) Acquire(ctx context.Context, teamID, jobID string, limit int) (bool, error) {
	if l.rdb == nil {
		return true, nil // No Redis → no limiting.
	}
	if limit <= 0 {
		limit = l.defaultLimit
	}

	key := l.key(teamID)
	now := float64(time.Now().UnixMilli())

	// Cleanup expired slots first.
	cutoff := float64(time.Now().Add(-l.slotTTL).UnixMilli())
	l.rdb.ZRemRangeByScore(ctx, key, "-inf", fmt.Sprintf("%f", cutoff))

	// Check current count.
	count, err := l.rdb.ZCard(ctx, key).Result()
	if err != nil {
		log.Warn().Err(err).Str("team", teamID).Msg("concurrency limiter: ZCard failed, allowing")
		return true, nil
	}
	if int(count) >= limit {
		return false, nil
	}

	// Add the job slot.
	added, err := l.rdb.ZAdd(ctx, key, redis.Z{
		Score:  now,
		Member: jobID,
	}).Result()
	if err != nil {
		log.Warn().Err(err).Str("team", teamID).Str("job", jobID).Msg("concurrency limiter: ZAdd failed")
		return true, nil // Fail open.
	}

	// Set key expiry as safety net.
	l.rdb.Expire(ctx, key, l.slotTTL+5*time.Minute)

	if added == 0 {
		// Job was already in the set (idempotent re-acquire).
		return true, nil
	}

	// Double-check we didn't exceed the limit in a race.
	newCount, _ := l.rdb.ZCard(ctx, key).Result()
	if int(newCount) > limit {
		// We went over; remove our slot and reject.
		l.rdb.ZRem(ctx, key, jobID)
		return false, nil
	}

	log.Debug().
		Str("team", teamID).
		Str("job", jobID).
		Int64("active", newCount).
		Int("limit", limit).
		Msg("concurrency slot acquired")

	return true, nil
}

// Release frees a concurrency slot for the given team+jobID.
func (l *Limiter) Release(ctx context.Context, teamID, jobID string) {
	if l.rdb == nil {
		return
	}
	key := l.key(teamID)
	removed, err := l.rdb.ZRem(ctx, key, jobID).Result()
	if err != nil {
		log.Warn().Err(err).Str("team", teamID).Str("job", jobID).Msg("concurrency limiter: release failed")
		return
	}
	if removed > 0 {
		log.Debug().Str("team", teamID).Str("job", jobID).Msg("concurrency slot released")
	}
}

// ActiveCount returns the number of active jobs for a team.
func (l *Limiter) ActiveCount(ctx context.Context, teamID string) (int64, error) {
	if l.rdb == nil {
		return 0, nil
	}
	key := l.key(teamID)
	// Cleanup expired slots first.
	cutoff := float64(time.Now().Add(-l.slotTTL).UnixMilli())
	l.rdb.ZRemRangeByScore(ctx, key, "-inf", fmt.Sprintf("%f", cutoff))
	return l.rdb.ZCard(ctx, key).Result()
}

// ActiveJobs returns the job IDs currently holding slots for a team.
func (l *Limiter) ActiveJobs(ctx context.Context, teamID string) ([]string, error) {
	if l.rdb == nil {
		return nil, nil
	}
	key := l.key(teamID)
	return l.rdb.ZRange(ctx, key, 0, -1).Result()
}

func (l *Limiter) key(teamID string) string {
	return l.keyPrefix + teamID
}

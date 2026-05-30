// Package history provides a Redis-backed store for per-domain engine scrape
// outcomes. It is the data layer used by the EngineAdvisor to learn which
// scraping engine works best for each domain.
package history

import (
	"context"
	"encoding/json"
	"fmt"
	"net/url"
	"time"

	"github.com/redis/go-redis/v9"
)

const (
	// keyPrefix namespaces all keys written by this package.
	keyPrefix = "quarry:eng:hist:"

	// recordTTL is how long we retain outcome data per domain.
	recordTTL = 30 * 24 * time.Hour // 30 days

	// maxRecordsPerEngine is the maximum number of outcome samples kept per
	// domain+engine pair (LPUSH + LTRIM keeps a sliding window).
	maxRecordsPerEngine = 50
)

// OutcomeRecord holds the result of a single scrape attempt.
type OutcomeRecord struct {
	Success      bool    `json:"ok"`
	LatencyMs    int     `json:"latency_ms"`
	QualityScore float32 `json:"quality"`
	Timestamp    int64   `json:"ts"` // Unix seconds
}

// engineKey returns the Redis key for a given domain+engine pair.
func engineKey(domain, engine string) string {
	return keyPrefix + domain + ":" + engine
}

// Store persists per-domain engine outcomes in Redis.
type Store struct {
	rdb *redis.Client
}

// NewStore creates a new Store backed by the provided Redis client.
func NewStore(rdb *redis.Client) *Store {
	return &Store{rdb: rdb}
}

// Record appends an outcome to the sliding window for domain+engine.
func (s *Store) Record(ctx context.Context, domain, engine string, rec OutcomeRecord) error {
	rec.Timestamp = time.Now().Unix()
	data, err := json.Marshal(rec)
	if err != nil {
		return fmt.Errorf("history marshal: %w", err)
	}

	key := engineKey(domain, engine)
	pipe := s.rdb.Pipeline()
	pipe.LPush(ctx, key, data)
	pipe.LTrim(ctx, key, 0, int64(maxRecordsPerEngine-1))
	pipe.Expire(ctx, key, recordTTL)
	_, err = pipe.Exec(ctx)
	return err
}

// Stats returns aggregated success rate (0–1) and average latency for a
// domain+engine pair. Returns zero values when no data exists.
func (s *Store) Stats(ctx context.Context, domain, engine string) (successRate float64, avgLatencyMs float64, n int) {
	key := engineKey(domain, engine)
	items, err := s.rdb.LRange(ctx, key, 0, int64(maxRecordsPerEngine-1)).Result()
	if err != nil || len(items) == 0 {
		return 0, 0, 0
	}

	var successes int
	var totalLatency int
	for _, item := range items {
		var rec OutcomeRecord
		if json.Unmarshal([]byte(item), &rec) != nil {
			continue
		}
		if rec.Success {
			successes++
		}
		totalLatency += rec.LatencyMs
	}
	n = len(items)
	successRate = float64(successes) / float64(n)
	avgLatencyMs = float64(totalLatency) / float64(n)
	return
}

// ExtractDomain extracts the registerable domain from a full URL for use as
// the Store key. Falls back to the host when parsing fails.
func ExtractDomain(rawURL string) string {
	u, err := url.Parse(rawURL)
	if err != nil || u.Host == "" {
		return rawURL
	}
	return u.Hostname()
}

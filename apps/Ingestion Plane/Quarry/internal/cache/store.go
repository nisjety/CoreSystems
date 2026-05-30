package cache

import (
	"context"
	"crypto/sha1"
	"encoding/hex"
	"encoding/json"
	"errors"
	"sort"
	"sync"
	"time"
)

var ErrCacheMiss = errors.New("cache miss")

type Stats struct {
	CreatedAt     time.Time `json:"created_at"`
	LastUpdated   time.Time `json:"last_updated"`
	TotalRequests int64     `json:"total_requests"`
	CacheHits     int64     `json:"cache_hits"`
	CacheMisses   int64     `json:"cache_misses"`
	HitRate       float64   `json:"hit_rate"`
	ItemCount     int64     `json:"item_count"`
	MemoryUsage   int64     `json:"memory_usage"`
	AverageSize   int64     `json:"average_size"`
}

type Store interface {
	Get(ctx context.Context, key string) (interface{}, error)
	Set(ctx context.Context, key string, value interface{}, ttl time.Duration) error
	Delete(ctx context.Context, key string) error
	Clear(ctx context.Context) error
	Exists(ctx context.Context, key string) (bool, error)
	GetStats(ctx context.Context) Stats
}

type MemoryStore struct {
	mu    sync.RWMutex
	items map[string]memoryItem
	stats Stats
}

type memoryItem struct {
	value     interface{}
	expiresAt time.Time
	size      int64
}

func NewMemoryStore() *MemoryStore {
	return &MemoryStore{
		items: make(map[string]memoryItem),
		stats: Stats{CreatedAt: time.Now(), LastUpdated: time.Now()},
	}
}

func (m *MemoryStore) Get(ctx context.Context, key string) (interface{}, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.stats.TotalRequests++
	item, ok := m.items[key]
	if !ok || time.Now().After(item.expiresAt) {
		delete(m.items, key)
		m.stats.CacheMisses++
		m.updateHitRate()
		return nil, ErrCacheMiss
	}
	m.stats.CacheHits++
	m.updateHitRate()
	return item.value, nil
}

func (m *MemoryStore) Set(ctx context.Context, key string, value interface{}, ttl time.Duration) error {
	if ttl <= 0 {
		ttl = 24 * time.Hour
	}
	data, _ := json.Marshal(value)
	m.mu.Lock()
	defer m.mu.Unlock()
	m.items[key] = memoryItem{value: value, expiresAt: time.Now().Add(ttl), size: int64(len(data))}
	m.recomputeStatsLocked()
	return nil
}

func (m *MemoryStore) Delete(ctx context.Context, key string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	delete(m.items, key)
	m.recomputeStatsLocked()
	return nil
}

func (m *MemoryStore) Clear(ctx context.Context) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.items = make(map[string]memoryItem)
	m.recomputeStatsLocked()
	return nil
}

func (m *MemoryStore) Exists(ctx context.Context, key string) (bool, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	item, ok := m.items[key]
	if !ok || time.Now().After(item.expiresAt) {
		return false, nil
	}
	return true, nil
}

func (m *MemoryStore) GetStats(ctx context.Context) Stats {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.stats
}

func (m *MemoryStore) updateHitRate() {
	if m.stats.TotalRequests == 0 {
		m.stats.HitRate = 0
		return
	}
	m.stats.HitRate = (float64(m.stats.CacheHits) / float64(m.stats.TotalRequests)) * 100
	m.stats.LastUpdated = time.Now()
}

func (m *MemoryStore) recomputeStatsLocked() {
	var total int64
	for _, item := range m.items {
		total += item.size
	}
	m.stats.ItemCount = int64(len(m.items))
	m.stats.MemoryUsage = total
	if m.stats.ItemCount > 0 {
		m.stats.AverageSize = total / m.stats.ItemCount
	} else {
		m.stats.AverageSize = 0
	}
	m.stats.LastUpdated = time.Now()
}

func GenerateKey(base string, params map[string]interface{}) string {
	h := sha1.New()
	_, _ = h.Write([]byte(base))
	if len(params) == 0 {
		return hex.EncodeToString(h.Sum(nil))
	}
	keys := make([]string, 0, len(params))
	for k := range params {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	normalized := make(map[string]interface{}, len(params))
	for _, k := range keys {
		normalized[k] = params[k]
	}
	b, _ := json.Marshal(normalized)
	_, _ = h.Write(b)
	return hex.EncodeToString(h.Sum(nil))
}

// Close allows MemoryStore to satisfy optional closer checks without special casing.
func (m *MemoryStore) Close() error {
	return nil
}

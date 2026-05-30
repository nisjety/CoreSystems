package api

import (
	"encoding/json"
	"sync"
	"time"
)

// SchemaCache stores compacted JSON schema strings to avoid repeated parsing/compaction.
type SchemaCache struct {
	mu    sync.RWMutex
	items map[string]schemaItem
	ttl   time.Duration
	done  chan struct{}
	once  sync.Once
}

type schemaItem struct {
	value     string
	updatedAt time.Time
}

// NewSchemaCache creates a new SchemaCache with the given TTL.
func NewSchemaCache(ttl time.Duration) *SchemaCache {
	if ttl <= 0 {
		ttl = time.Hour
	}
	c := &SchemaCache{
		items: make(map[string]schemaItem),
		ttl:   ttl,
		done:  make(chan struct{}),
	}
	go c.cleanupLoop()
	return c
}

// Get returns the compacted schema string if present.
func (c *SchemaCache) Get(key string) (string, bool) {
	c.mu.RLock()
	it, ok := c.items[key]
	if !ok || time.Since(it.updatedAt) <= c.ttl {
		defer c.mu.RUnlock()
		if !ok {
			return "", false
		}
		return it.value, true
	}
	c.mu.RUnlock()

	// Expired entries are evicted on read so stale schemas do not survive until
	// the periodic cleanup tick.
	c.mu.Lock()
	defer c.mu.Unlock()
	it, ok = c.items[key]
	if !ok || time.Since(it.updatedAt) > c.ttl {
		delete(c.items, key)
		return "", false
	}
	return it.value, true
}

// Set stores a compacted schema string in the cache.
func (c *SchemaCache) Set(key, compact string) {
	c.mu.Lock()
	c.items[key] = schemaItem{value: compact, updatedAt: time.Now()}
	c.mu.Unlock()
}

// cleanupLoop removes expired entries periodically.
func (c *SchemaCache) cleanupLoop() {
	ticker := time.NewTicker(5 * time.Minute)
	defer ticker.Stop()
	for {
		select {
		case <-ticker.C:
			now := time.Now()
			c.mu.Lock()
			for k, it := range c.items {
				if now.Sub(it.updatedAt) > c.ttl {
					delete(c.items, k)
				}
			}
			c.mu.Unlock()
		case <-c.done:
			return
		}
	}
}

// Close stops the cleanup goroutine.
func (c *SchemaCache) Close() error {
	c.once.Do(func() {
		close(c.done)
	})
	return nil
}

// CompactJSON returns a compacted representation of a JSON string.
func CompactJSON(src string) (string, error) {
	var v interface{}
	if err := json.Unmarshal([]byte(src), &v); err != nil {
		return "", err
	}
	b, err := json.Marshal(v)
	if err != nil {
		return "", err
	}
	return string(b), nil
}

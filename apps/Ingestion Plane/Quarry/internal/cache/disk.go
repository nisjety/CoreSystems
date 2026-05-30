package cache

import (
	"context"
	"encoding/json"
	"fmt"
	"io/ioutil"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/rs/zerolog/log"
)

// DiskStore implements file-based caching with persistence
type DiskStore struct {
	mu       sync.RWMutex
	basePath string
	stats    Stats
	ticker   *time.Ticker
	stopChan chan struct{}
	closeOnce sync.Once
}

// CacheFile represents the structure of cached files
type CacheFile struct {
	Key       string        `json:"key"`
	Data      interface{}   `json:"data"`
	StoredAt  time.Time     `json:"stored_at"`
	ExpiresAt time.Time     `json:"expires_at"`
	TTL       time.Duration `json:"ttl"`
}

// NewDiskStore creates a new disk-based cache store
func NewDiskStore(basePath string) (*DiskStore, error) {
	// Create base directory if it doesn't exist
	if err := os.MkdirAll(basePath, 0755); err != nil {
		return nil, fmt.Errorf("failed to create cache directory: %w", err)
	}

	store := &DiskStore{
		basePath: basePath,
		stopChan: make(chan struct{}),
		stats: Stats{
			CreatedAt:   time.Now(),
			LastUpdated: time.Now(),
		},
	}

	// Start cleanup ticker for expired entries
	store.ticker = time.NewTicker(5 * time.Minute)
	go func() {
		for {
			select {
			case <-store.ticker.C:
				store.cleanupExpired()
			case <-store.stopChan:
				return
			}
		}
	}()

	return store, nil
}

// Get retrieves a value from disk cache
func (d *DiskStore) Get(ctx context.Context, key string) (interface{}, error) {
	d.mu.RLock()
	defer d.mu.RUnlock()

	d.stats.TotalRequests++

	filePath := filepath.Join(d.basePath, key+".json")
	data, err := ioutil.ReadFile(filePath)
	if err != nil {
		d.stats.CacheMisses++
		d.stats.HitRate = float64(d.stats.CacheHits) / float64(d.stats.TotalRequests) * 100
		return nil, fmt.Errorf("cache miss: %w", err)
	}

	var cacheFile CacheFile
	if err := json.Unmarshal(data, &cacheFile); err != nil {
		d.stats.CacheMisses++
		d.stats.HitRate = float64(d.stats.CacheHits) / float64(d.stats.TotalRequests) * 100
		return nil, fmt.Errorf("failed to unmarshal cache file: %w", err)
	}

	// Check if expired
	if time.Now().After(cacheFile.ExpiresAt) {
		os.Remove(filePath)
		d.stats.CacheMisses++
		d.stats.HitRate = float64(d.stats.CacheHits) / float64(d.stats.TotalRequests) * 100
		return nil, fmt.Errorf("cache miss: entry expired")
	}

	d.stats.CacheHits++
	d.stats.HitRate = float64(d.stats.CacheHits) / float64(d.stats.TotalRequests) * 100
	d.stats.LastUpdated = time.Now()

	log.Debug().
		Str("key", key).
		Msg("Disk cache hit")

	return cacheFile.Data, nil
}

// Set stores a value on disk with TTL
func (d *DiskStore) Set(ctx context.Context, key string, value interface{}, ttl time.Duration) error {
	d.mu.Lock()
	defer d.mu.Unlock()

	if ttl == 0 {
		ttl = 2 * 24 * time.Hour // Default 2 days
	}

	cacheFile := CacheFile{
		Key:       key,
		Data:      value,
		StoredAt:  time.Now(),
		ExpiresAt: time.Now().Add(ttl),
		TTL:       ttl,
	}

	data, err := json.MarshalIndent(cacheFile, "", "  ")
	if err != nil {
		return fmt.Errorf("failed to marshal cache file: %w", err)
	}

	filePath := filepath.Join(d.basePath, key+".json")
	if err := ioutil.WriteFile(filePath, data, 0644); err != nil {
		return fmt.Errorf("failed to write cache file: %w", err)
	}

	d.stats.LastUpdated = time.Now()
	d.updateStats()

	log.Debug().
		Str("key", key).
		Int("size", len(data)).
		Dur("ttl", ttl).
		Msg("Disk cache set")

	return nil
}

// Delete removes an entry from disk cache
func (d *DiskStore) Delete(ctx context.Context, key string) error {
	d.mu.Lock()
	defer d.mu.Unlock()

	filePath := filepath.Join(d.basePath, key+".json")
	if err := os.Remove(filePath); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("failed to delete cache file: %w", err)
	}

	d.stats.LastUpdated = time.Now()
	return nil
}

// Clear removes all entries from disk cache
func (d *DiskStore) Clear(ctx context.Context) error {
	d.mu.Lock()
	defer d.mu.Unlock()

	files, err := ioutil.ReadDir(d.basePath)
	if err != nil {
		return fmt.Errorf("failed to read cache directory: %w", err)
	}

	for _, file := range files {
		if file.Name() != "" {
			os.Remove(filepath.Join(d.basePath, file.Name()))
		}
	}

	d.stats.LastUpdated = time.Now()
	return nil
}

// Exists checks if a key exists on disk and is not expired
func (d *DiskStore) Exists(ctx context.Context, key string) (bool, error) {
	d.mu.RLock()
	defer d.mu.RUnlock()

	filePath := filepath.Join(d.basePath, key+".json")
	data, err := ioutil.ReadFile(filePath)
	if err != nil {
		return false, nil
	}

	var cacheFile CacheFile
	if err := json.Unmarshal(data, &cacheFile); err != nil {
		return false, nil
	}

	if time.Now().After(cacheFile.ExpiresAt) {
		return false, nil
	}

	return true, nil
}

// GetStats returns cache statistics
func (d *DiskStore) GetStats(ctx context.Context) Stats {
	d.mu.RLock()
	defer d.mu.RUnlock()

	d.updateStats()
	return d.stats
}

// updateStats recalculates cache statistics
func (d *DiskStore) updateStats() {
	files, err := ioutil.ReadDir(d.basePath)
	if err != nil {
		return
	}

	d.stats.ItemCount = int64(len(files))
	totalSize := int64(0)

	for _, file := range files {
		totalSize += file.Size()
	}

	d.stats.MemoryUsage = totalSize
	if d.stats.ItemCount > 0 {
		d.stats.AverageSize = totalSize / d.stats.ItemCount
	}
}

// cleanupExpired removes expired entries from disk
func (d *DiskStore) cleanupExpired() {
	d.mu.Lock()
	defer d.mu.Unlock()

	files, err := ioutil.ReadDir(d.basePath)
	if err != nil {
		return
	}

	now := time.Now()
	deleted := 0

	for _, file := range files {
		filePath := filepath.Join(d.basePath, file.Name())
		data, err := ioutil.ReadFile(filePath)
		if err != nil {
			continue
		}

		var cacheFile CacheFile
		if err := json.Unmarshal(data, &cacheFile); err != nil {
			continue
		}

		if now.After(cacheFile.ExpiresAt) {
			os.Remove(filePath)
			deleted++
		}
	}

	if deleted > 0 {
		log.Info().
			Int("deleted", deleted).
			Msg("Disk cache cleanup completed")
	}

	d.updateStats()
}

// Close stops the cleanup ticker
func (d *DiskStore) Close() error {
	d.closeOnce.Do(func() {
		if d.ticker != nil {
			d.ticker.Stop()
		}
		close(d.stopChan)
	})
	return nil
}

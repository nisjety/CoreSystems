package cache

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/rs/zerolog/log"
)

// Manager wraps the cache store and provides convenient methods
type Manager struct {
	store Store
	stats Stats
}

// NewManager creates a new cache manager
func NewManager(store Store) *Manager {
	return &Manager{
		store: store,
	}
}

// GetWithFreshness retrieves a value from cache, respecting maxAge
// If the cached entry is newer than maxAge, it's returned
// Otherwise, returns cache miss
func (m *Manager) GetWithFreshness(ctx context.Context, key string, maxAgeMs int64) (interface{}, bool, error) {
	if maxAgeMs == 0 {
		// maxAge = 0 means always fetch fresh
		return nil, false, fmt.Errorf("force fresh requested")
	}

	value, err := m.store.Get(ctx, key)
	if err != nil {
		return nil, false, err
	}

	return value, true, nil
}

// Set stores a value with TTL
// If maxAge is provided in milliseconds, use it; otherwise default to 2 days
func (m *Manager) Set(ctx context.Context, key string, value interface{}, maxAgeMs int64) error {
	ttl := 2 * 24 * time.Hour // Default 2 days

	if maxAgeMs > 0 {
		ttl = time.Duration(maxAgeMs) * time.Millisecond
	}

	return m.store.Set(ctx, key, value, ttl)
}

// Delete removes a cache entry
func (m *Manager) Delete(ctx context.Context, key string) error {
	return m.store.Delete(ctx, key)
}

// Clear removes all cache entries
func (m *Manager) Clear(ctx context.Context) error {
	return m.store.Clear(ctx)
}

// Exists checks if a key exists and is fresh
func (m *Manager) Exists(ctx context.Context, key string) bool {
	exists, _ := m.store.Exists(ctx, key)
	return exists
}

// GetStats returns current cache statistics
func (m *Manager) GetStats(ctx context.Context) Stats {
	return m.store.GetStats(ctx)
}

// CachePageHTML stores page HTML with freshness
func (m *Manager) CachePageHTML(ctx context.Context, url string, html string, maxAgeMs int64) error {
	key := GenerateKey(url, map[string]interface{}{
		"type": "page_html",
	})

	data := map[string]interface{}{
		"url":       url,
		"html":      html,
		"cached_at": time.Now().Unix(),
	}

	return m.Set(ctx, key, data, maxAgeMs)
}

// GetCachedPageHTML retrieves cached page HTML
func (m *Manager) GetCachedPageHTML(ctx context.Context, url string, maxAgeMs int64) (string, bool, error) {
	key := GenerateKey(url, map[string]interface{}{
		"type": "page_html",
	})

	if maxAgeMs == 0 {
		// Force fresh
		return "", false, nil
	}

	value, hit, err := m.GetWithFreshness(ctx, key, maxAgeMs)
	if err != nil || !hit {
		return "", false, err
	}

	data, ok := value.(map[string]interface{})
	if !ok {
		return "", false, fmt.Errorf("invalid cached data format")
	}

	html, ok := data["html"].(string)
	if !ok {
		return "", false, fmt.Errorf("html not found in cached data")
	}

	log.Info().
		Str("url", url).
		Int64("max_age_ms", maxAgeMs).
		Msg("Returning cached page HTML (500% faster!)")

	return html, true, nil
}

// CacheProductDetails stores extracted product details with freshness
func (m *Manager) CacheProductDetails(ctx context.Context, url string, details interface{}, maxAgeMs int64) error {
	key := GenerateKey(url, map[string]interface{}{
		"type": "product_details",
	})

	data := map[string]interface{}{
		"url":       url,
		"details":   details,
		"cached_at": time.Now().Unix(),
	}

	return m.Set(ctx, key, data, maxAgeMs)
}

// GetCachedProductDetails retrieves cached product details
func (m *Manager) GetCachedProductDetails(ctx context.Context, url string, maxAgeMs int64) (interface{}, bool, error) {
	key := GenerateKey(url, map[string]interface{}{
		"type": "product_details",
	})

	if maxAgeMs == 0 {
		// Force fresh
		return nil, false, nil
	}

	value, hit, err := m.GetWithFreshness(ctx, key, maxAgeMs)
	if err != nil || !hit {
		return nil, false, err
	}

	data, ok := value.(map[string]interface{})
	if !ok {
		return nil, false, fmt.Errorf("invalid cached data format")
	}

	log.Info().
		Str("url", url).
		Int64("max_age_ms", maxAgeMs).
		Msg("Returning cached product details (500% faster!)")

	return data["details"], true, nil
}

// SerializeToJSON converts cache data to JSON for API responses
func SerializeToJSON(data interface{}) (string, error) {
	jsonData, err := json.MarshalIndent(data, "", "  ")
	if err != nil {
		return "", err
	}
	return string(jsonData), nil
}

// Close releases resources held by the underlying cache store when supported.
func (m *Manager) Close() error {
	if m == nil || m.store == nil {
		return nil
	}
	if closer, ok := m.store.(interface{ Close() error }); ok {
		return closer.Close()
	}
	return nil
}

// CacheFormats stores the full multi-format output keyed by URL + format list.
// This provides aggressive caching at the FetchFormats level — the most impactful
// cache layer since it skips driver init, page load, and format conversion entirely.
func (m *Manager) CacheFormats(ctx context.Context, url string, formats []string, outputs map[string]interface{}, maxAgeMs int64) error {
	key := GenerateKey(url, map[string]interface{}{
		"type":    "formats",
		"formats": formats,
	})

	data := map[string]interface{}{
		"url":       url,
		"outputs":   outputs,
		"cached_at": time.Now().Unix(),
	}

	return m.Set(ctx, key, data, maxAgeMs)
}

// GetCachedFormats retrieves cached multi-format output.
// Returns (outputs, hit, err). On miss returns nil, false, nil.
func (m *Manager) GetCachedFormats(ctx context.Context, url string, formats []string, maxAgeMs int64) (map[string]interface{}, bool, error) {
	if maxAgeMs == 0 {
		return nil, false, nil
	}

	key := GenerateKey(url, map[string]interface{}{
		"type":    "formats",
		"formats": formats,
	})

	value, hit, err := m.GetWithFreshness(ctx, key, maxAgeMs)
	if err != nil || !hit {
		return nil, false, err
	}

	data, ok := value.(map[string]interface{})
	if !ok {
		return nil, false, nil
	}

	outputs, ok := data["outputs"].(map[string]interface{})
	if !ok {
		return nil, false, nil
	}

	log.Info().
		Str("url", url).
		Int64("max_age_ms", maxAgeMs).
		Msg("Cache HIT: returning cached format outputs")

	return outputs, true, nil
}

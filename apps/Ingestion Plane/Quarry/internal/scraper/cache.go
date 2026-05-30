package scraper

import (
	"context"
	"fmt"

	"github.com/rs/zerolog/log"
	"github.com/triodelab/quarry/internal/cache"
)

// ScrapeCachedPage fetches a page with cache support
// Returns (html, hitFromCache, error)
func (s *Scraper) ScrapeCachedPage(ctx context.Context, url string, maxAgeMs int64) (string, bool, error) {
	// Try to get from cache first if maxAge allows
	if s.cacheManager != nil && maxAgeMs != 0 {
		if cached, hit, _ := s.cacheManager.GetCachedPageHTML(ctx, url, maxAgeMs); hit {
			log.Info().
				Str("url", url).
				Int64("max_age_ms", maxAgeMs).
				Msg("Cache hit - returning cached HTML (500% faster!)")
			return cached, true, nil
		}
	}

	// Fetch fresh page
	log.Info().
		Str("url", url).
		Msg("Cache miss or force fresh - scraping live")

	page, cleanup, err := s.browserPool.GetPage(ctx)
	if err != nil {
		return "", false, fmt.Errorf("failed to get page from pool: %w", err)
	}
	defer cleanup()

	// Navigate to page
	if err := page.Navigate(url); err != nil {
		return "", false, fmt.Errorf("navigation failed: %w", err)
	}

	// Wait for page load
	if err := page.WaitLoad(); err != nil {
		return "", false, fmt.Errorf("wait load failed: %w", err)
	}

	// Get HTML content
	html, err := page.HTML()
	if err != nil {
		return "", false, fmt.Errorf("failed to get HTML: %w", err)
	}

	// Cache the result for future requests
	if s.cacheManager != nil && maxAgeMs != 0 {
		if err := s.cacheManager.CachePageHTML(ctx, url, html, maxAgeMs); err != nil {
			log.Warn().Err(err).Msg("Failed to cache page HTML")
		}
	}

	return html, false, nil
}

// SetCacheManager attaches a cache manager to the scraper
func (s *Scraper) SetCacheManager(cm *cache.Manager) {
	s.cacheManager = cm
}

package scraper

import (
	"fmt"
	"strings"

	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/cache"
	"github.com/triodelab/quarry/internal/config"
)

func newCacheManager(cfg *config.Config) (*cache.Manager, error) {
	if cfg == nil {
		return cache.NewManager(cache.NewMemoryStore()), nil
	}

	switch strings.ToLower(strings.TrimSpace(cfg.CacheBackend)) {
	case "", "memory":
		return cache.NewManager(cache.NewMemoryStore()), nil
	case "disk":
		store, err := cache.NewDiskStore(cfg.CachePath)
		if err != nil {
			return nil, fmt.Errorf("initialize disk cache: %w", err)
		}
		return cache.NewManager(store), nil
	case "redis":
		if strings.TrimSpace(cfg.RedisURL) == "" {
			return nil, fmt.Errorf("cache backend is redis but REDIS_URL is not configured")
		}
		store, err := cache.NewRedisStore(cfg.RedisURL)
		if err != nil {
			return nil, fmt.Errorf("initialize redis cache: %w", err)
		}
		return cache.NewManager(store), nil
	default:
		log.Warn().Str("backend", cfg.CacheBackend).Msg("unknown cache backend, falling back to memory")
		return cache.NewManager(cache.NewMemoryStore()), nil
	}
}

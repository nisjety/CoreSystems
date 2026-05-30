package platform

import (
	"strconv"
	"sync"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"
)

// TierLimits defines per-tier rate-limit configuration.
type TierLimits struct {
	MaxRequests int           // max requests per window
	Window      time.Duration // sliding window
}

// DefaultTierLimits returns the built-in limit table.
// Override via TierRateLimiterConfig.Tiers.
func DefaultTierLimits() map[string]TierLimits {
	return map[string]TierLimits{
		"free":       {MaxRequests: 10, Window: 60 * time.Second},
		"starter":    {MaxRequests: 30, Window: 60 * time.Second},
		"pro":        {MaxRequests: 60, Window: 60 * time.Second},
		"enterprise": {MaxRequests: 300, Window: 60 * time.Second},
		"internal":   {MaxRequests: 1000, Window: 60 * time.Second}, // service-to-service
	}
}

// TierRateLimiterConfig configures the tier-aware rate limiter.
type TierRateLimiterConfig struct {
	Tiers map[string]TierLimits
	// FallbackMax is applied when the tier is unknown.
	FallbackMax    int
	FallbackWindow time.Duration
}

// ─── sliding window bucket ───────────────────────────────────────────────────

type bucket struct {
	mu       sync.Mutex
	hits     []time.Time
	max      int
	window   time.Duration
	lastTier string
}

func (b *bucket) allow(now time.Time) bool {
	b.mu.Lock()
	defer b.mu.Unlock()

	// Evict expired entries.
	cutoff := now.Add(-b.window)
	i := 0
	for i < len(b.hits) && b.hits[i].Before(cutoff) {
		i++
	}
	b.hits = b.hits[i:]

	if len(b.hits) >= b.max {
		return false
	}
	b.hits = append(b.hits, now)
	return true
}

func (b *bucket) remaining() int {
	b.mu.Lock()
	defer b.mu.Unlock()

	cutoff := time.Now().Add(-b.window)
	active := 0
	for _, t := range b.hits {
		if !t.Before(cutoff) {
			active++
		}
	}
	r := b.max - active
	if r < 0 {
		r = 0
	}
	return r
}

// ─── limiter store ───────────────────────────────────────────────────────────

type tierLimiterStore struct {
	mu      sync.RWMutex
	buckets map[string]*bucket // keyed by orgID (or "ip:<addr>" for unauthenticated)
}

func newTierLimiterStore() *tierLimiterStore {
	return &tierLimiterStore{
		buckets: make(map[string]*bucket),
	}
}

func (s *tierLimiterStore) getOrCreate(key, tier string, limits TierLimits) *bucket {
	s.mu.RLock()
	b, ok := s.buckets[key]
	s.mu.RUnlock()

	if ok {
		// If tier changed (e.g. org upgraded), hot-swap limits.
		b.mu.Lock()
		if b.lastTier != tier {
			b.max = limits.MaxRequests
			b.window = limits.Window
			b.lastTier = tier
		}
		b.mu.Unlock()
		return b
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	// Double-check after acquiring write lock.
	if b, ok := s.buckets[key]; ok {
		return b
	}

	b = &bucket{
		hits:     make([]time.Time, 0, limits.MaxRequests),
		max:      limits.MaxRequests,
		window:   limits.Window,
		lastTier: tier,
	}
	s.buckets[key] = b
	return b
}

// ─── middleware ───────────────────────────────────────────────────────────────

// TierRateLimiter returns a Fiber middleware that enforces per-org, per-tier
// rate limits. It must be installed AFTER AuthBridgeMiddleware so that
// the Principal is available in c.Locals.
func TierRateLimiter(cfg TierRateLimiterConfig) fiber.Handler {
	if cfg.Tiers == nil {
		cfg.Tiers = DefaultTierLimits()
	}
	if cfg.FallbackMax == 0 {
		cfg.FallbackMax = 10
	}
	if cfg.FallbackWindow == 0 {
		cfg.FallbackWindow = 60 * time.Second
	}

	store := newTierLimiterStore()

	return func(c *fiber.Ctx) error {
		principal := GetPrincipal(c)

		var key, tier string
		if principal != nil {
			key = "org:" + principal.OrganizationID
			tier = principal.Tier
		} else {
			key = "ip:" + c.IP()
			tier = "free"
		}

		limits, ok := cfg.Tiers[tier]
		if !ok {
			limits = TierLimits{
				MaxRequests: cfg.FallbackMax,
				Window:      cfg.FallbackWindow,
			}
		}

		b := store.getOrCreate(key, tier, limits)
		now := time.Now()

		if !b.allow(now) {
			log.Warn().
				Str("event", "tier_rate_limit_exceeded").
				Str("key", key).
				Str("tier", tier).
				Int("max", limits.MaxRequests).
				Str("path", c.Path()).
				Msg("tier-based rate limit triggered")

c.Set("X-RateLimit-Limit", strconv.Itoa(limits.MaxRequests))
		c.Set("X-RateLimit-Remaining", "0")
		c.Set("Retry-After", strconv.Itoa(int(limits.Window.Seconds())))

			return c.Status(fiber.StatusTooManyRequests).JSON(fiber.Map{
				"success":   false,
				"error":     "rate limit exceeded",
				"tier":      tier,
				"limit":     limits.MaxRequests,
				"windowSec": int(limits.Window.Seconds()),
				"requestId": c.GetRespHeader("X-Request-ID"),
			})
		}

		// Set rate-limit headers for transparency.
		c.Set("X-RateLimit-Limit", strconv.Itoa(limits.MaxRequests))
		c.Set("X-RateLimit-Remaining", strconv.Itoa(b.remaining()))

		return c.Next()
	}
}

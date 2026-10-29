package ratelimit

import (
	"context"
	"fmt"
	"net/http"
	"sync"
	"time"

	"github.com/coresystem/integration-gateway/internal/audit"
	"github.com/coresystem/integration-gateway/internal/cache"
	"golang.org/x/time/rate"
)

// Limiter provides rate limiting functionality
type Limiter struct {
	globalLimiter *rate.Limiter
	adminLimiter  *rate.Limiter
	cache         *cache.RedisCache
	audit         *audit.AuditLogger
	distributed   bool
	mu            sync.RWMutex
}

// Config holds rate limiter configuration
type Config struct {
	GlobalRPS   int
	AdminRPS    int
	BurstSize   int
	Distributed bool
	Cache       *cache.RedisCache
	AuditLogger *audit.AuditLogger
}

// New creates a new rate limiter
func New(cfg Config) *Limiter {
	return &Limiter{
		globalLimiter: rate.NewLimiter(rate.Limit(cfg.GlobalRPS), cfg.BurstSize),
		adminLimiter:  rate.NewLimiter(rate.Limit(cfg.AdminRPS), cfg.BurstSize/2),
		cache:         cfg.Cache,
		audit:         cfg.AuditLogger,
		distributed:   cfg.Distributed,
	}
}

// AllowGlobal checks if a global request is allowed
func (l *Limiter) AllowGlobal(ctx context.Context) bool {
	if l.distributed && l.cache != nil {
		return l.allowDistributed(ctx, "global", l.globalLimiter.Limit())
	}
	return l.globalLimiter.Allow()
}

// AllowAdmin checks if an admin request is allowed
func (l *Limiter) AllowAdmin(ctx context.Context) bool {
	if l.distributed && l.cache != nil {
		return l.allowDistributed(ctx, "admin", l.adminLimiter.Limit())
	}
	return l.adminLimiter.Allow()
}

// allowDistributed implements distributed rate limiting using Redis
func (l *Limiter) allowDistributed(ctx context.Context, tier string, limit rate.Limit) bool {
	key := fmt.Sprintf("ratelimit:%s:%d", tier, time.Now().Unix()/60) // Per-minute window

	// Increment counter
	count, err := l.cache.Increment(key)
	if err != nil {
		// Fall back to local limiter on Redis error
		if tier == "admin" {
			return l.adminLimiter.Allow()
		}
		return l.globalLimiter.Allow()
	}

	// Set expiration on first request
	if count == 1 {
		_ = l.cache.Expire(key, 2*time.Minute) // 2-minute window for safety
	}

	// Check if limit exceeded
	return count <= int64(limit)*60 // Convert per-second to per-minute
}

// Middleware returns a middleware that enforces global rate limiting
func (l *Limiter) Middleware(tier string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			var allowed bool

			switch tier {
			case "admin":
				allowed = l.AllowAdmin(r.Context())
			default:
				allowed = l.AllowGlobal(r.Context())
			}

			if !allowed {
				// Log rate limit violation
				if l.audit != nil {
					l.audit.LogRateLimitExceeded(r, tier)
				}

				w.Header().Set("X-RateLimit-Limit", fmt.Sprintf("%d", l.getLimit(tier)))
				w.Header().Set("X-RateLimit-Remaining", "0")
				w.Header().Set("Retry-After", "60")
				w.WriteHeader(http.StatusTooManyRequests)
				w.Write([]byte(`{"error":"rate_limit_exceeded","message":"Too many requests"}`))
				return
			}

			// Add rate limit headers
			w.Header().Set("X-RateLimit-Limit", fmt.Sprintf("%d", l.getLimit(tier)))

			next.ServeHTTP(w, r)
		})
	}
}

// getLimit returns the rate limit for a tier
func (l *Limiter) getLimit(tier string) int {
	if tier == "admin" {
		return int(l.adminLimiter.Limit())
	}
	return int(l.globalLimiter.Limit())
}

// PerIPLimiter provides per-IP rate limiting
type PerIPLimiter struct {
	limiters map[string]*rate.Limiter
	mu       sync.RWMutex
	limit    rate.Limit
	burst    int
}

// NewPerIPLimiter creates a new per-IP rate limiter
func NewPerIPLimiter(rps int, burst int) *PerIPLimiter {
	return &PerIPLimiter{
		limiters: make(map[string]*rate.Limiter),
		limit:    rate.Limit(rps),
		burst:    burst,
	}
}

// GetLimiter returns the rate limiter for an IP
func (pl *PerIPLimiter) GetLimiter(ip string) *rate.Limiter {
	pl.mu.Lock()
	defer pl.mu.Unlock()

	limiter, exists := pl.limiters[ip]
	if !exists {
		limiter = rate.NewLimiter(pl.limit, pl.burst)
		pl.limiters[ip] = limiter
	}

	return limiter
}

// CleanupStale removes old limiters (call periodically)
func (pl *PerIPLimiter) CleanupStale() {
	pl.mu.Lock()
	defer pl.mu.Unlock()

	// In production, implement proper cleanup based on last access time
	// For now, clear all if too many
	if len(pl.limiters) > 10000 {
		pl.limiters = make(map[string]*rate.Limiter)
	}
}

// PerIPMiddleware returns a middleware that enforces per-IP rate limiting
func (pl *PerIPLimiter) Middleware(audit *audit.AuditLogger) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			// Get IP from request
			ip := getIP(r)
			limiter := pl.GetLimiter(ip)

			if !limiter.Allow() {
				// Log rate limit violation
				if audit != nil {
					audit.LogRateLimitExceeded(r, "per-ip")
				}

				w.Header().Set("X-RateLimit-Limit", fmt.Sprintf("%d", pl.limit))
				w.Header().Set("X-RateLimit-Remaining", "0")
				w.Header().Set("Retry-After", "60")
				w.WriteHeader(http.StatusTooManyRequests)
				w.Write([]byte(`{"error":"rate_limit_exceeded","message":"Too many requests from your IP"}`))
				return
			}

			next.ServeHTTP(w, r)
		})
	}
}

// getIP extracts the real IP from request
func getIP(r *http.Request) string {
	// Check X-Forwarded-For header first (Traefik sets this)
	if xff := r.Header.Get("X-Forwarded-For"); xff != "" {
		return xff
	}

	// Check X-Real-IP header
	if xri := r.Header.Get("X-Real-IP"); xri != "" {
		return xri
	}

	// Fall back to RemoteAddr
	return r.RemoteAddr
}

// Stats returns rate limiter statistics
type Stats struct {
	GlobalLimit int    `json:"global_limit"`
	AdminLimit  int    `json:"admin_limit"`
	Distributed bool   `json:"distributed"`
	Backend     string `json:"backend"`
}

// GetStats returns current rate limiter statistics
func (l *Limiter) GetStats() Stats {
	backend := "local"
	if l.distributed && l.cache != nil {
		backend = "redis"
	}

	return Stats{
		GlobalLimit: int(l.globalLimiter.Limit()),
		AdminLimit:  int(l.adminLimiter.Limit()),
		Distributed: l.distributed,
		Backend:     backend,
	}
}

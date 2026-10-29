// Package server provides browser-broker gRPC interceptors and handlers.
package server

import (
	"context"
	"sync"
	"time"

	"github.com/triodelab/model-plane/services/browser-broker/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/peer"
	"google.golang.org/grpc/status"
)

// rateLimitBucket tracks request count within a fixed time window.
type rateLimitBucket struct {
	count       int
	windowStart time.Time
}

// rateLimiter enforces a fixed-window per-key request limit.
type rateLimiter struct {
	mu      sync.Mutex
	limit   int
	window  time.Duration
	buckets map[string]*rateLimitBucket
}

// allow reports whether a request for the given key is permitted at time now.
func (r *rateLimiter) allow(key string, now time.Time) bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	b, ok := r.buckets[key]
	if !ok || now.Sub(b.windowStart) >= r.window {
		r.buckets[key] = &rateLimitBucket{count: 1, windowStart: now}
		return true
	}
	if b.count >= r.limit {
		return false
	}
	b.count++
	return true
}

// rateLimitKey extracts a per-caller key from ctx, falling back to "unknown".
func rateLimitKey(ctx context.Context) string {
	if p, ok := peer.FromContext(ctx); ok && p != nil && p.Addr != nil {
		return p.Addr.String()
	}
	return "unknown"
}

// UnaryRateLimitInterceptor returns a gRPC unary interceptor that enforces a
// per-peer fixed-window rate limit at the gateway boundary. Requests exceeding
// the limit are rejected with codes.ResourceExhausted and counted via
// telemetry.GatewayRateLimitedTotal.
func UnaryRateLimitInterceptor(limit int, window time.Duration) grpc.UnaryServerInterceptor {
	rl := &rateLimiter{
		limit:   limit,
		window:  window,
		buckets: make(map[string]*rateLimitBucket),
	}
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {
		if !rl.allow(rateLimitKey(ctx), time.Now()) {
			telemetry.GatewayRateLimitedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", info.FullMethod)))
			return nil, status.Error(codes.ResourceExhausted, "rate limit exceeded")
		}
		return handler(ctx, req)
	}
}

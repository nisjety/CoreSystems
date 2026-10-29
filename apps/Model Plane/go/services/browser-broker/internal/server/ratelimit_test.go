package server

import (
	"context"
	"net"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/peer"
	"google.golang.org/grpc/status"
)

func peerCtx(ip string, port int) context.Context {
	return peer.NewContext(context.Background(), &peer.Peer{
		Addr: &net.TCPAddr{IP: net.ParseIP(ip), Port: port},
	})
}

func okHandler(_ context.Context, _ any) (any, error) { return "ok", nil }

func TestRateLimit_AllowsUnderLimit(t *testing.T) {
	interceptor := UnaryRateLimitInterceptor(3, time.Minute)
	info := &grpc.UnaryServerInfo{FullMethod: "/test/Method"}
	ctx := peerCtx("10.0.0.1", 1234)

	for i := 0; i < 3; i++ {
		resp, err := interceptor(ctx, nil, info, okHandler)
		if err != nil {
			t.Fatalf("req %d: unexpected error: %v", i, err)
		}
		if resp != "ok" {
			t.Fatalf("req %d: expected handler response, got %v", i, resp)
		}
	}
}

func TestRateLimit_RejectsOverLimit_ReturnsResourceExhausted(t *testing.T) {
	interceptor := UnaryRateLimitInterceptor(2, time.Minute)
	info := &grpc.UnaryServerInfo{FullMethod: "/test/Method"}
	ctx := peerCtx("10.0.0.2", 1234)

	for i := 0; i < 2; i++ {
		if _, err := interceptor(ctx, nil, info, okHandler); err != nil {
			t.Fatalf("req %d should succeed, got %v", i, err)
		}
	}
	_, err := interceptor(ctx, nil, info, okHandler)
	if err == nil {
		t.Fatal("expected error on 3rd request, got nil")
	}
	if code := status.Code(err); code != codes.ResourceExhausted {
		t.Fatalf("expected codes.ResourceExhausted, got %v", code)
	}
}

func TestRateLimit_PerKeyIsolation(t *testing.T) {
	interceptor := UnaryRateLimitInterceptor(1, time.Minute)
	info := &grpc.UnaryServerInfo{FullMethod: "/test/Method"}
	ctxA := peerCtx("10.0.0.3", 1111)
	ctxB := peerCtx("10.0.0.4", 2222)

	if _, err := interceptor(ctxA, nil, info, okHandler); err != nil {
		t.Fatalf("peer A first req: %v", err)
	}
	if _, err := interceptor(ctxB, nil, info, okHandler); err != nil {
		t.Fatalf("peer B first req should not be rate-limited: %v", err)
	}
	if _, err := interceptor(ctxA, nil, info, okHandler); status.Code(err) != codes.ResourceExhausted {
		t.Fatalf("peer A second req should be ResourceExhausted, got %v", err)
	}
}

func TestRateLimit_WindowResets(t *testing.T) {
	rl := &rateLimiter{
		limit:   1,
		window:  10 * time.Millisecond,
		buckets: make(map[string]*rateLimitBucket),
	}
	t0 := time.Now()
	if !rl.allow("k", t0) {
		t.Fatal("first request should be allowed")
	}
	if rl.allow("k", t0.Add(5*time.Millisecond)) {
		t.Fatal("second request within window should be denied")
	}
	if !rl.allow("k", t0.Add(20*time.Millisecond)) {
		t.Fatal("request after window should be allowed")
	}
}

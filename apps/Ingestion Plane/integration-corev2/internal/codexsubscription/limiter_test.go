package codexsubscription

import (
	"context"
	"errors"
	"sync"
	"testing"
	"time"
)

func acquireAsync(ctx context.Context, limiter *invocationLimiter, connectionID string) chan func() {
	acquired := make(chan func(), 1)
	go func() {
		release, _, err := limiter.acquire(ctx, connectionID)
		if err != nil {
			close(acquired)
			return
		}
		acquired <- release
	}()
	return acquired
}

func expectBlocked(t *testing.T, acquired chan func(), what string) {
	t.Helper()
	select {
	case release, ok := <-acquired:
		if ok {
			release()
		}
		t.Fatalf("%s was not blocked", what)
	case <-time.After(80 * time.Millisecond):
	}
}

func expectAcquired(t *testing.T, acquired chan func(), what string) func() {
	t.Helper()
	select {
	case release, ok := <-acquired:
		if !ok {
			t.Fatalf("%s failed to acquire", what)
		}
		return release
	case <-time.After(time.Second):
		t.Fatalf("%s stayed blocked", what)
	}
	return nil
}

func TestConcurrencyLimits(t *testing.T) {
	ctx := t.Context()
	limiter := newInvocationLimiter(8, 2, 5*time.Second)
	first, _, err := limiter.acquire(ctx, "conn-a")
	if err != nil {
		t.Fatal(err)
	}
	second, _, err := limiter.acquire(ctx, "conn-a")
	if err != nil {
		t.Fatal(err)
	}
	third := acquireAsync(ctx, limiter, "conn-a")
	expectBlocked(t, third, "a third call on one connection")
	other := expectAcquired(t, acquireAsync(ctx, limiter, "conn-b"), "another connection")
	first()
	first() // release is idempotent
	thirdRelease := expectAcquired(t, third, "the queued call")
	second()
	thirdRelease()
	other()
	if limiter.trackedConnections() != 0 {
		t.Fatalf("tracked connections = %d, want 0 after every release", limiter.trackedConnections())
	}

	// The global cap blocks across connections.
	global := newInvocationLimiter(2, 2, 5*time.Second)
	a, _, _ := global.acquire(ctx, "conn-a")
	b, _, _ := global.acquire(ctx, "conn-b")
	blocked := acquireAsync(ctx, global, "conn-c")
	expectBlocked(t, blocked, "a call over the global cap")
	a()
	release := expectAcquired(t, blocked, "the call after a global release")
	b()
	release()

	// The queue timeout is a broker_queue rate limit with a 1s retry.
	queued := newInvocationLimiter(1, 1, 50*time.Millisecond)
	hold, _, _ := queued.acquire(ctx, "conn-a")
	_, _, err = queued.acquire(ctx, "conn-a")
	var limited *RateLimitError
	if !errors.As(err, &limited) || limited.Kind != RateLimitQueue || limited.RetryAfter != time.Second {
		t.Fatalf("queue timeout error = %v", err)
	}
	hold()

	// An invocation deadline shorter than the queue timeout wins.
	deadlineCtx, cancel := context.WithTimeout(ctx, 30*time.Millisecond)
	defer cancel()
	hold, _, _ = queued.acquire(ctx, "conn-a")
	queued.queueTimeout = time.Minute
	if _, _, err := queued.acquire(deadlineCtx, "conn-a"); !errors.As(err, &limited) && !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("deadline error = %v", err)
	}
	hold()
	if queued.trackedConnections() != 0 {
		t.Fatal("a timed-out waiter leaked its connection entry")
	}
}

type blockingRunner struct {
	fakeRunner
	mu      sync.Mutex
	active  int
	peak    int
	release chan struct{}
	panicOn string
}

func (b *blockingRunner) Invoke(_ context.Context, _ string, request InvokeRequest) (InvokeResponse, error) {
	b.mu.Lock()
	b.active++
	b.peak = max(b.peak, b.active)
	b.mu.Unlock()
	defer func() {
		b.mu.Lock()
		b.active--
		b.mu.Unlock()
	}()
	if request.RequestID == b.panicOn {
		panic("runner exploded")
	}
	if request.RequestID == "fail" {
		return InvokeResponse{}, errors.New("upstream failed")
	}
	<-b.release
	return InvokeResponse{RequestID: request.RequestID, Content: "ok", ModelUsed: request.Model}, nil
}

func TestManagerLimiterReleasesAfterErrorsAndPanics(t *testing.T) {
	runner := &blockingRunner{release: make(chan struct{}), panicOn: "boom"}
	close(runner.release)
	manager, err := NewManager(Config{Enabled: true, Home: t.TempDir(), MaxConcurrency: 1, MaxConcurrencyPerConnection: 1, QueueTimeout: 100 * time.Millisecond}, runner)
	if err != nil {
		t.Fatal(err)
	}
	request := InvokeRequest{ConnectionID: "conn-1", RequestID: "fail", Model: "gpt-5.6-terra", Messages: []ChatMessage{{Role: "user", Content: "hi"}}}
	if _, err := manager.Invoke(t.Context(), request); err == nil {
		t.Fatal("want the runner error")
	}
	func() {
		defer func() { _ = recover() }()
		request.RequestID = "boom"
		_, _ = manager.Invoke(t.Context(), request)
	}()
	request.RequestID = "ok"
	if _, err := manager.Invoke(t.Context(), request); err != nil {
		t.Fatalf("a slot leaked after an error or panic: %v", err)
	}
	if manager.limiter.trackedConnections() != 0 {
		t.Fatal("connection entries leaked")
	}
}

func TestManagerQueueTimeoutIsRateLimit(t *testing.T) {
	runner := &blockingRunner{release: make(chan struct{})}
	manager, err := NewManager(Config{Enabled: true, Home: t.TempDir(), MaxConcurrency: 4, MaxConcurrencyPerConnection: 1, QueueTimeout: 50 * time.Millisecond}, runner)
	if err != nil {
		t.Fatal(err)
	}
	request := InvokeRequest{ConnectionID: "conn-1", RequestID: "held", Model: "gpt-5.6-terra", Messages: []ChatMessage{{Role: "user", Content: "hi"}}}
	done := make(chan struct{})
	go func() {
		_, _ = manager.Invoke(context.Background(), request)
		close(done)
	}()
	deadline := time.Now().Add(time.Second)
	for {
		runner.mu.Lock()
		active := runner.active
		runner.mu.Unlock()
		if active == 1 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("the first call never started")
		}
		time.Sleep(5 * time.Millisecond)
	}
	request.RequestID = "queued"
	_, err = manager.Invoke(t.Context(), request)
	if !errors.Is(err, ErrUpstreamRateLimited) {
		t.Fatalf("error = %v, want a broker_queue rate limit", err)
	}
	close(runner.release)
	<-done
}

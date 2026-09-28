package codexsubscription

import (
	"context"
	"sync"
	"time"
)

const (
	defaultMaxConcurrency              = 16
	defaultMaxConcurrencyPerConnection = 4
	defaultQueueTimeout                = 30 * time.Second
	queueRetryAfter                    = time.Second
)

// invocationLimiter caps concurrent calls globally and per connection. It is
// transport-agnostic, so it also caps app-server spawns. A caller that cannot
// get both slots within the queue deadline gets a broker_queue rate limit.
type invocationLimiter struct {
	global        chan struct{}
	perConnection int
	queueTimeout  time.Duration

	mu          sync.Mutex
	connections map[string]*connectionSlots
}

// connectionSlots is one connection's semaphore and the number of callers
// holding or waiting for it; the entry is dropped when that reaches zero.
type connectionSlots struct {
	sem  chan struct{}
	refs int
}

func newInvocationLimiter(global, perConnection int, queueTimeout time.Duration) *invocationLimiter {
	if global <= 0 {
		global = defaultMaxConcurrency
	}
	if perConnection <= 0 {
		perConnection = defaultMaxConcurrencyPerConnection
	}
	perConnection = min(perConnection, global)
	if queueTimeout <= 0 {
		queueTimeout = defaultQueueTimeout
	}
	return &invocationLimiter{
		global:        make(chan struct{}, global),
		perConnection: perConnection,
		queueTimeout:  queueTimeout,
		connections:   make(map[string]*connectionSlots),
	}
}

// acquire takes a per-connection slot, then a global one, under a queue
// deadline of min(queueTimeout, what ctx has left). release is idempotent.
func (l *invocationLimiter) acquire(ctx context.Context, connectionID string) (func(), time.Duration, error) {
	started := time.Now()
	queueTimeout := l.queueTimeout
	if deadline, ok := ctx.Deadline(); ok {
		queueTimeout = min(queueTimeout, time.Until(deadline))
	}
	if err := ctx.Err(); err != nil {
		return nil, 0, err
	}
	if queueTimeout <= 0 {
		return nil, 0, &RateLimitError{Kind: RateLimitQueue, RetryAfter: queueRetryAfter}
	}
	timer := time.NewTimer(queueTimeout)
	defer timer.Stop()

	slots := l.retain(connectionID)
	if err := waitForSlot(ctx, timer, slots.sem); err != nil {
		l.drop(connectionID, slots)
		return nil, 0, err
	}
	if err := waitForSlot(ctx, timer, l.global); err != nil {
		<-slots.sem
		l.drop(connectionID, slots)
		return nil, 0, err
	}
	var once sync.Once
	release := func() {
		once.Do(func() {
			<-l.global
			<-slots.sem
			l.drop(connectionID, slots)
		})
	}
	return release, time.Since(started), nil
}

func waitForSlot(ctx context.Context, timer *time.Timer, sem chan struct{}) error {
	select {
	case sem <- struct{}{}:
		return nil
	default:
	}
	select {
	case sem <- struct{}{}:
		return nil
	case <-timer.C:
		return &RateLimitError{Kind: RateLimitQueue, RetryAfter: queueRetryAfter}
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (l *invocationLimiter) retain(connectionID string) *connectionSlots {
	l.mu.Lock()
	defer l.mu.Unlock()
	slots, ok := l.connections[connectionID]
	if !ok {
		slots = &connectionSlots{sem: make(chan struct{}, l.perConnection)}
		l.connections[connectionID] = slots
	}
	slots.refs++
	return slots
}

func (l *invocationLimiter) drop(connectionID string, slots *connectionSlots) {
	l.mu.Lock()
	defer l.mu.Unlock()
	slots.refs--
	if slots.refs <= 0 && l.connections[connectionID] == slots {
		delete(l.connections, connectionID)
	}
}

// trackedConnections is the number of connections with a live entry.
func (l *invocationLimiter) trackedConnections() int {
	l.mu.Lock()
	defer l.mu.Unlock()
	return len(l.connections)
}

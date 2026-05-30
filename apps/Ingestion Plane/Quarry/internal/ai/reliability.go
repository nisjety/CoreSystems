package ai

import (
	"context"
	"errors"
	"sync"
	"time"
)

var ErrCircuitOpen = errors.New("ai circuit breaker is open")

type CircuitState string

const (
	CircuitClosed   CircuitState = "closed"
	CircuitOpen     CircuitState = "open"
	CircuitHalfOpen CircuitState = "half_open"
)

type HealthMonitor struct {
	client      AIClient
	interval    time.Duration
	probeTimout time.Duration

	mu          sync.RWMutex
	healthy     bool
	lastError   string
	lastChecked time.Time
	lastLatency time.Duration

	stop chan struct{}
}

func NewHealthMonitor(client AIClient, interval, probeTimeout time.Duration) *HealthMonitor {
	if interval <= 0 {
		interval = 15 * time.Second
	}
	if probeTimeout <= 0 {
		probeTimeout = 2 * time.Second
	}

	return &HealthMonitor{
		client:      client,
		interval:    interval,
		probeTimout: probeTimeout,
		healthy:     client != nil,
		stop:        make(chan struct{}),
	}
}

func (m *HealthMonitor) Start() {
	if m == nil || m.client == nil {
		return
	}

	go func() {
		m.probe()
		ticker := time.NewTicker(m.interval)
		defer ticker.Stop()
		for {
			select {
			case <-ticker.C:
				m.probe()
			case <-m.stop:
				return
			}
		}
	}()
}

func (m *HealthMonitor) Stop() {
	if m == nil {
		return
	}
	select {
	case <-m.stop:
		return
	default:
		close(m.stop)
	}
}

func (m *HealthMonitor) probe() {
	start := time.Now()
	ctx, cancel := context.WithTimeout(context.Background(), m.probeTimout)
	defer cancel()

	// Lightweight health probe: classify an empty payload.
	_, err := m.client.ClassifyContent(ctx, &ClassifyRequest{HTML: "<html></html>", URL: "health-probe"})
	latency := time.Since(start)

	m.mu.Lock()
	defer m.mu.Unlock()
	m.lastChecked = time.Now()
	m.lastLatency = latency
	if err != nil {
		m.healthy = false
		m.lastError = err.Error()
		return
	}
	m.healthy = true
	m.lastError = ""
}

func (m *HealthMonitor) Snapshot() map[string]interface{} {
	if m == nil {
		return map[string]interface{}{"enabled": false}
	}
	m.mu.RLock()
	defer m.mu.RUnlock()
	return map[string]interface{}{
		"enabled":           true,
		"healthy":           m.healthy,
		"last_error":        m.lastError,
		"last_checked_unix": m.lastChecked.Unix(),
		"last_latency_ms":   float64(m.lastLatency) / float64(time.Millisecond),
	}
}

type CircuitBreaker struct {
	mu sync.RWMutex

	state                CircuitState
	consecutiveFailures  int
	halfOpenAttempts     int
	halfOpenSuccesses    int
	openUntil            time.Time
	failureThreshold     int
	halfOpenMaxRequests  int
	halfOpenSuccessCount int
	openDuration         time.Duration
}

func NewCircuitBreaker() *CircuitBreaker {
	return &CircuitBreaker{
		state:                CircuitClosed,
		failureThreshold:     5,
		halfOpenMaxRequests:  2,
		halfOpenSuccessCount: 2,
		openDuration:         30 * time.Second,
	}
}

func (b *CircuitBreaker) BeforeAttempt() error {
	if b == nil {
		return nil
	}

	b.mu.Lock()
	defer b.mu.Unlock()

	now := time.Now()
	switch b.state {
	case CircuitClosed:
		return nil
	case CircuitOpen:
		if now.Before(b.openUntil) {
			return ErrCircuitOpen
		}
		b.state = CircuitHalfOpen
		b.halfOpenAttempts = 1
		b.halfOpenSuccesses = 0
		return nil
	case CircuitHalfOpen:
		if b.halfOpenAttempts >= b.halfOpenMaxRequests {
			return ErrCircuitOpen
		}
		b.halfOpenAttempts++
		return nil
	default:
		return nil
	}
}

func (b *CircuitBreaker) AfterAttempt(success bool) {
	if b == nil {
		return
	}

	b.mu.Lock()
	defer b.mu.Unlock()

	switch b.state {
	case CircuitClosed:
		if success {
			b.consecutiveFailures = 0
			return
		}
		b.consecutiveFailures++
		if b.consecutiveFailures >= b.failureThreshold {
			b.state = CircuitOpen
			b.openUntil = time.Now().Add(b.openDuration)
		}
	case CircuitHalfOpen:
		if success {
			b.halfOpenSuccesses++
			if b.halfOpenSuccesses >= b.halfOpenSuccessCount {
				b.state = CircuitClosed
				b.consecutiveFailures = 0
				b.halfOpenAttempts = 0
				b.halfOpenSuccesses = 0
			}
			return
		}
		b.state = CircuitOpen
		b.openUntil = time.Now().Add(b.openDuration)
		b.halfOpenAttempts = 0
		b.halfOpenSuccesses = 0
	}
}

func (b *CircuitBreaker) Snapshot() map[string]interface{} {
	if b == nil {
		return map[string]interface{}{"enabled": false}
	}
	b.mu.RLock()
	defer b.mu.RUnlock()
	return map[string]interface{}{
		"enabled":                true,
		"state":                  string(b.state),
		"consecutive_failures":   b.consecutiveFailures,
		"open_until_unix":        b.openUntil.Unix(),
		"half_open_attempts":     b.halfOpenAttempts,
		"half_open_successes":    b.halfOpenSuccesses,
		"failure_threshold":      b.failureThreshold,
		"half_open_max_requests": b.halfOpenMaxRequests,
	}
}

type SLOTracker struct {
	mu sync.RWMutex

	total          int64
	success        int64
	withinLatency  int64
	totalLatencyNs int64
	latencyBudget  time.Duration
}

func NewSLOTracker(latencyBudget time.Duration) *SLOTracker {
	if latencyBudget <= 0 {
		latencyBudget = 5 * time.Second
	}
	return &SLOTracker{latencyBudget: latencyBudget}
}

func (s *SLOTracker) Record(success bool, latency time.Duration) {
	if s == nil {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.total++
	if success {
		s.success++
	}
	if latency <= s.latencyBudget {
		s.withinLatency++
	}
	s.totalLatencyNs += latency.Nanoseconds()
}

func (s *SLOTracker) Snapshot() map[string]interface{} {
	if s == nil {
		return map[string]interface{}{"enabled": false}
	}
	s.mu.RLock()
	defer s.mu.RUnlock()

	successRate := 0.0
	latencyBudgetRate := 0.0
	avgLatencyMs := 0.0
	if s.total > 0 {
		successRate = float64(s.success) / float64(s.total)
		latencyBudgetRate = float64(s.withinLatency) / float64(s.total)
		avgLatencyMs = float64(s.totalLatencyNs) / float64(s.total) / float64(time.Millisecond)
	}

	return map[string]interface{}{
		"enabled":             true,
		"total":               s.total,
		"success":             s.success,
		"success_rate":        successRate,
		"latency_budget_ms":   float64(s.latencyBudget) / float64(time.Millisecond),
		"latency_budget_rate": latencyBudgetRate,
		"avg_latency_ms":      avgLatencyMs,
	}
}

type ReliabilityGate struct {
	breaker *CircuitBreaker
	health  *HealthMonitor
	slo     *SLOTracker
}

func NewReliabilityGate(client AIClient) *ReliabilityGate {
	if client == nil {
		return nil
	}
	health := NewHealthMonitor(client, 15*time.Second, 2*time.Second)
	health.Start()
	return &ReliabilityGate{
		breaker: NewCircuitBreaker(),
		health:  health,
		slo:     NewSLOTracker(5 * time.Second),
	}
}

func (g *ReliabilityGate) BeforeAttempt() error {
	if g == nil {
		return nil
	}
	return g.breaker.BeforeAttempt()
}

func (g *ReliabilityGate) AfterAttempt(success bool, latency time.Duration) {
	if g == nil {
		return
	}
	g.breaker.AfterAttempt(success)
	g.slo.Record(success, latency)
}

func (g *ReliabilityGate) Snapshot() map[string]interface{} {
	if g == nil {
		return map[string]interface{}{"enabled": false}
	}
	return map[string]interface{}{
		"enabled": true,
		"health":  g.health.Snapshot(),
		"circuit": g.breaker.Snapshot(),
		"slo":     g.slo.Snapshot(),
	}
}

func (g *ReliabilityGate) Close() {
	if g == nil {
		return
	}
	g.health.Stop()
}

package circuitbreaker

import (
	"context"
	"fmt"
	"time"

	"github.com/sony/gobreaker/v2"
)

// Config holds circuit breaker configuration
type Config struct {
	Name             string
	MaxRequests      uint32
	Interval         time.Duration
	Timeout          time.Duration
	FailureThreshold uint32
	OnStateChange    func(name string, from gobreaker.State, to gobreaker.State)
}

// CircuitBreaker wraps gobreaker with additional functionality
type CircuitBreaker struct {
	cb     *gobreaker.CircuitBreaker[[]byte]
	config Config
}

// New creates a new circuit breaker
func New(config Config) *CircuitBreaker {
	settings := gobreaker.Settings{
		Name:        config.Name,
		MaxRequests: config.MaxRequests,
		Interval:    config.Interval,
		Timeout:     config.Timeout,
		ReadyToTrip: func(counts gobreaker.Counts) bool {
			return counts.ConsecutiveFailures >= config.FailureThreshold
		},
		OnStateChange: config.OnStateChange,
		IsSuccessful: func(err error) bool {
			// All non-nil errors are considered failures
			return err == nil
		},
	}

	return &CircuitBreaker{
		cb:     gobreaker.NewCircuitBreaker[[]byte](settings),
		config: config,
	}
}

// Execute runs the given function through the circuit breaker
func (cb *CircuitBreaker) Execute(fn func() ([]byte, error)) ([]byte, error) {
	return cb.cb.Execute(fn)
}

// ExecuteContext runs the given function through the circuit breaker with context
func (cb *CircuitBreaker) ExecuteContext(ctx context.Context, fn func() ([]byte, error)) ([]byte, error) {
	// Check context before executing
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}

	// Create a channel for the result
	type result struct {
		data []byte
		err  error
	}
	resultChan := make(chan result, 1)

	// Execute in goroutine
	go func() {
		data, err := cb.Execute(fn)
		resultChan <- result{data: data, err: err}
	}()

	// Wait for result or context cancellation
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case res := <-resultChan:
		return res.data, res.err
	}
}

// State returns the current state of the circuit breaker
func (cb *CircuitBreaker) State() gobreaker.State {
	return cb.cb.State()
}

// Name returns the name of the circuit breaker
func (cb *CircuitBreaker) Name() string {
	return cb.config.Name
}

// Counts returns the current counts
func (cb *CircuitBreaker) Counts() gobreaker.Counts {
	return cb.cb.Counts()
}

// StateString returns the state as a string
func StateString(state gobreaker.State) string {
	switch state {
	case gobreaker.StateClosed:
		return "closed"
	case gobreaker.StateOpen:
		return "open"
	case gobreaker.StateHalfOpen:
		return "half-open"
	default:
		return "unknown"
	}
}

// Manager manages multiple circuit breakers for different services
type Manager struct {
	breakers map[string]*CircuitBreaker
}

// NewManager creates a new circuit breaker manager
func NewManager() *Manager {
	return &Manager{
		breakers: make(map[string]*CircuitBreaker),
	}
}

// Add adds a circuit breaker to the manager
func (m *Manager) Add(name string, cb *CircuitBreaker) {
	m.breakers[name] = cb
}

// Get retrieves a circuit breaker by name
func (m *Manager) Get(name string) (*CircuitBreaker, error) {
	cb, ok := m.breakers[name]
	if !ok {
		return nil, fmt.Errorf("circuit breaker not found: %s", name)
	}
	return cb, nil
}

// GetAll returns all circuit breakers
func (m *Manager) GetAll() map[string]*CircuitBreaker {
	return m.breakers
}

// GetStats returns statistics for all circuit breakers
func (m *Manager) GetStats() map[string]CircuitBreakerStats {
	stats := make(map[string]CircuitBreakerStats)
	for name, cb := range m.breakers {
		counts := cb.Counts()
		stats[name] = CircuitBreakerStats{
			Name:                 name,
			State:                StateString(cb.State()),
			TotalRequests:        counts.Requests,
			TotalSuccesses:       counts.TotalSuccesses,
			TotalFailures:        counts.TotalFailures,
			ConsecutiveSuccesses: counts.ConsecutiveSuccesses,
			ConsecutiveFailures:  counts.ConsecutiveFailures,
		}
	}
	return stats
}

// CircuitBreakerStats holds statistics for a circuit breaker
type CircuitBreakerStats struct {
	Name                 string `json:"name"`
	State                string `json:"state"`
	TotalRequests        uint32 `json:"total_requests"`
	TotalSuccesses       uint32 `json:"total_successes"`
	TotalFailures        uint32 `json:"total_failures"`
	ConsecutiveSuccesses uint32 `json:"consecutive_successes"`
	ConsecutiveFailures  uint32 `json:"consecutive_failures"`
}

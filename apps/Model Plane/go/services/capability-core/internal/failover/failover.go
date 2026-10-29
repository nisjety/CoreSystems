// Package failover codifies the provider-failover contract: circuit-breaker
// states, retry policy, and provider health schema. It intentionally contains
// no runtime wiring — shim-only — so the contract can be verified in isolation
// and consumed by the Rust model-gateway / Go capability-core services without
// importing transport code.
package failover

import (
	"errors"
	"time"
)

// CircuitState enumerates the three valid circuit-breaker states.
type CircuitState string

const (
	StateClosed   CircuitState = "closed"
	StateOpen     CircuitState = "open"
	StateHalfOpen CircuitState = "half_open"
)

// Valid reports whether s is a known circuit state.
func (s CircuitState) Valid() bool {
	switch s {
	case StateClosed, StateOpen, StateHalfOpen:
		return true
	}
	return false
}

// CanTransitionTo enforces the legal state transition graph:
//
//	closed    -> open
//	open      -> half_open
//	half_open -> closed, open
func (s CircuitState) CanTransitionTo(next CircuitState) bool {
	if !s.Valid() || !next.Valid() {
		return false
	}
	switch s {
	case StateClosed:
		return next == StateOpen
	case StateOpen:
		return next == StateHalfOpen
	case StateHalfOpen:
		return next == StateClosed || next == StateOpen
	}
	return false
}

// RetryPolicy governs per-request retry behavior.
type RetryPolicy struct {
	MaxAttempts  int           `json:"max_attempts"`
	InitialDelay time.Duration `json:"initial_delay"`
	MaxDelay     time.Duration `json:"max_delay"`
	Multiplier   float64       `json:"multiplier"`
	JitterRatio  float64       `json:"jitter_ratio"`
}

// Validate enforces RetryPolicy invariants.
func (p RetryPolicy) Validate() error {
	if p.MaxAttempts < 1 {
		return errors.New("failover: MaxAttempts must be >= 1")
	}
	if p.InitialDelay <= 0 {
		return errors.New("failover: InitialDelay must be > 0")
	}
	if p.MaxDelay < p.InitialDelay {
		return errors.New("failover: MaxDelay must be >= InitialDelay")
	}
	if p.Multiplier < 1.0 {
		return errors.New("failover: Multiplier must be >= 1.0 (monotonic backoff)")
	}
	if p.JitterRatio < 0 || p.JitterRatio > 1 {
		return errors.New("failover: JitterRatio must be in [0,1]")
	}
	return nil
}

// FailoverConfig is the top-level contract config.
type FailoverConfig struct {
	Retry               RetryPolicy   `json:"retry"`
	FailureThreshold    int           `json:"failure_threshold"`
	SuccessThreshold    int           `json:"success_threshold"`
	OpenStateCooldown   time.Duration `json:"open_state_cooldown"`
	HalfOpenProbeBudget int           `json:"half_open_probe_budget"`
}

// Validate enforces FailoverConfig invariants.
func (c FailoverConfig) Validate() error {
	if err := c.Retry.Validate(); err != nil {
		return err
	}
	if c.FailureThreshold < 1 {
		return errors.New("failover: FailureThreshold must be >= 1")
	}
	if c.SuccessThreshold < 1 {
		return errors.New("failover: SuccessThreshold must be >= 1")
	}
	if c.OpenStateCooldown <= 0 {
		return errors.New("failover: OpenStateCooldown must be > 0")
	}
	if c.HalfOpenProbeBudget < 1 {
		return errors.New("failover: HalfOpenProbeBudget must be >= 1")
	}
	return nil
}

// ProviderHealth is the observability record the registry exposes per provider.
type ProviderHealth struct {
	ProviderID       string       `json:"provider_id"`
	State            CircuitState `json:"state"`
	ConsecutiveFails int          `json:"consecutive_fails"`
	ConsecutivePass  int          `json:"consecutive_pass"`
	LastTransition   time.Time    `json:"last_transition"`
}

// Validate enforces ProviderHealth invariants.
func (h ProviderHealth) Validate() error {
	if h.ProviderID == "" {
		return errors.New("failover: ProviderID is required")
	}
	if !h.State.Valid() {
		return errors.New("failover: invalid State")
	}
	if h.ConsecutiveFails < 0 || h.ConsecutivePass < 0 {
		return errors.New("failover: consecutive counters must be >= 0")
	}
	return nil
}

// DefaultFailoverConfig returns the baseline contract defaults consumed by
// downstream services when no override is supplied.
func DefaultFailoverConfig() FailoverConfig {
	return FailoverConfig{
		Retry: RetryPolicy{
			MaxAttempts:  3,
			InitialDelay: 100 * time.Millisecond,
			MaxDelay:     2 * time.Second,
			Multiplier:   2.0,
			JitterRatio:  0.2,
		},
		FailureThreshold:    5,
		SuccessThreshold:    2,
		OpenStateCooldown:   30 * time.Second,
		HalfOpenProbeBudget: 1,
	}
}

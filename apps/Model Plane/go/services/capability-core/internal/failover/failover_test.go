package failover

import (
	"encoding/json"
	"strings"
	"testing"
	"time"
)

func TestCircuitStateValid(t *testing.T) {
	for _, s := range []CircuitState{StateClosed, StateOpen, StateHalfOpen} {
		if !s.Valid() {
			t.Errorf("expected %q valid", s)
		}
	}
	if CircuitState("bogus").Valid() {
		t.Error("unknown state must not be valid")
	}
}

func TestCircuitStateTransitions(t *testing.T) {
	legal := map[CircuitState][]CircuitState{
		StateClosed:   {StateOpen},
		StateOpen:     {StateHalfOpen},
		StateHalfOpen: {StateClosed, StateOpen},
	}
	all := []CircuitState{StateClosed, StateOpen, StateHalfOpen}
	for from, oks := range legal {
		okSet := map[CircuitState]bool{}
		for _, o := range oks {
			okSet[o] = true
		}
		for _, to := range all {
			got := from.CanTransitionTo(to)
			want := okSet[to]
			if got != want {
				t.Errorf("%s->%s: got %v want %v", from, to, got, want)
			}
		}
	}
	if StateClosed.CanTransitionTo(CircuitState("bogus")) {
		t.Error("transition to invalid state must be rejected")
	}
}

func TestRetryPolicyValidate(t *testing.T) {
	good := RetryPolicy{MaxAttempts: 3, InitialDelay: 10 * time.Millisecond, MaxDelay: time.Second, Multiplier: 2.0, JitterRatio: 0.1}
	if err := good.Validate(); err != nil {
		t.Fatalf("good policy rejected: %v", err)
	}
	cases := []struct {
		name string
		p    RetryPolicy
		msg  string
	}{
		{"zero attempts", RetryPolicy{MaxAttempts: 0, InitialDelay: 1, MaxDelay: 1, Multiplier: 1, JitterRatio: 0}, "MaxAttempts"},
		{"zero initial", RetryPolicy{MaxAttempts: 1, InitialDelay: 0, MaxDelay: 1, Multiplier: 1, JitterRatio: 0}, "InitialDelay"},
		{"non-monotonic", RetryPolicy{MaxAttempts: 1, InitialDelay: time.Second, MaxDelay: time.Millisecond, Multiplier: 1, JitterRatio: 0}, "MaxDelay"},
		{"shrinking multiplier", RetryPolicy{MaxAttempts: 1, InitialDelay: 1, MaxDelay: 1, Multiplier: 0.5, JitterRatio: 0}, "Multiplier"},
		{"jitter out of range", RetryPolicy{MaxAttempts: 1, InitialDelay: 1, MaxDelay: 1, Multiplier: 1, JitterRatio: 1.5}, "JitterRatio"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			err := c.p.Validate()
			if err == nil || !strings.Contains(err.Error(), c.msg) {
				t.Errorf("want error containing %q, got %v", c.msg, err)
			}
		})
	}
}

func TestFailoverConfigValidate(t *testing.T) {
	if err := DefaultFailoverConfig().Validate(); err != nil {
		t.Fatalf("default config invalid: %v", err)
	}
	bad := DefaultFailoverConfig()
	bad.FailureThreshold = 0
	if err := bad.Validate(); err == nil {
		t.Error("FailureThreshold=0 must be rejected")
	}
	bad = DefaultFailoverConfig()
	bad.OpenStateCooldown = 0
	if err := bad.Validate(); err == nil {
		t.Error("OpenStateCooldown=0 must be rejected")
	}
}

func TestProviderHealthValidate(t *testing.T) {
	h := ProviderHealth{ProviderID: "openai", State: StateClosed}
	if err := h.Validate(); err != nil {
		t.Fatalf("valid health rejected: %v", err)
	}
	if err := (ProviderHealth{State: StateClosed}).Validate(); err == nil {
		t.Error("missing ProviderID must be rejected")
	}
	if err := (ProviderHealth{ProviderID: "x", State: "bogus"}).Validate(); err == nil {
		t.Error("invalid state must be rejected")
	}
	if err := (ProviderHealth{ProviderID: "x", State: StateClosed, ConsecutiveFails: -1}).Validate(); err == nil {
		t.Error("negative counter must be rejected")
	}
}

func TestJSONStability(t *testing.T) {
	cfg := DefaultFailoverConfig()
	b, err := json.Marshal(cfg)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	for _, key := range []string{"max_attempts", "initial_delay", "jitter_ratio", "failure_threshold", "success_threshold", "open_state_cooldown", "half_open_probe_budget"} {
		if !strings.Contains(string(b), key) {
			t.Errorf("JSON missing stable key %q: %s", key, b)
		}
	}
	h := ProviderHealth{ProviderID: "p", State: StateHalfOpen}
	b, err = json.Marshal(h)
	if err != nil {
		t.Fatalf("marshal health: %v", err)
	}
	for _, key := range []string{"provider_id", "state", "consecutive_fails", "last_transition"} {
		if !strings.Contains(string(b), key) {
			t.Errorf("health JSON missing %q: %s", key, b)
		}
	}
}

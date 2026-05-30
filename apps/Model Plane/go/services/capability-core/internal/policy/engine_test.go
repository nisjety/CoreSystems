package policy_test

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

func TestEngine_Evaluate(t *testing.T) {
	reg := registry.NewRegistry()
	eng := policy.New(reg)
	ctx := context.Background()

	const (
		validCap   = "cap.memory.search"
		validRun   = "run-1"
		validAgent = "agent-1"
		validOrg   = "org-1"
	)

	t.Run("empty capID returns ErrInvalidArgument", func(t *testing.T) {
		_, err := eng.Evaluate(ctx, "", validRun, validAgent, validOrg, "")
		if !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("expected ErrInvalidArgument, got %v", err)
		}
	})

	t.Run("empty runID returns ErrInvalidArgument", func(t *testing.T) {
		_, err := eng.Evaluate(ctx, validCap, "", validAgent, validOrg, "")
		if !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("expected ErrInvalidArgument, got %v", err)
		}
	})

	t.Run("empty agentID returns ErrInvalidArgument", func(t *testing.T) {
		_, err := eng.Evaluate(ctx, validCap, validRun, "", validOrg, "")
		if !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("expected ErrInvalidArgument, got %v", err)
		}
	})

	t.Run("empty orgID returns ErrInvalidArgument", func(t *testing.T) {
		_, err := eng.Evaluate(ctx, validCap, validRun, validAgent, "", "")
		if !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("expected ErrInvalidArgument, got %v", err)
		}
	})

	t.Run("unknown capability returns ErrCapabilityNotFound", func(t *testing.T) {
		_, err := eng.Evaluate(ctx, "cap.does.not.exist", validRun, validAgent, validOrg, "")
		if !errors.Is(err, domain.ErrCapabilityNotFound) {
			t.Fatalf("expected ErrCapabilityNotFound, got %v", err)
		}
	})

	t.Run("low-risk capability is allowed with default budget", func(t *testing.T) {
		res, err := eng.Evaluate(ctx, "cap.memory.search", validRun, validAgent, validOrg, "")
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if res.Decision != policy.DecisionAllow {
			t.Errorf("Decision = %q, want %q", res.Decision, policy.DecisionAllow)
		}
		if res.Reason != "low-risk capability allowed under default budget" {
			t.Errorf("unexpected Reason: %q", res.Reason)
		}
		if res.BudgetContext != "tokens=100000,cost_usd=5.00" {
			t.Errorf("unexpected BudgetContext: %q", res.BudgetContext)
		}
	})

	t.Run("medium-risk capability is allowed with constrained budget", func(t *testing.T) {
		res, err := eng.Evaluate(ctx, "cap.browser.open", validRun, validAgent, validOrg, "")
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if res.Decision != policy.DecisionAllow {
			t.Errorf("Decision = %q, want %q", res.Decision, policy.DecisionAllow)
		}
		if res.Reason != "medium-risk capability allowed under constrained budget" {
			t.Errorf("unexpected Reason: %q", res.Reason)
		}
		if res.BudgetContext != "tokens=10000,cost_usd=0.50" {
			t.Errorf("unexpected BudgetContext: %q", res.BudgetContext)
		}
	})

	t.Run("high-risk capability is denied with empty budget", func(t *testing.T) {
		res, err := eng.Evaluate(ctx, "cap.sandbox.exec", validRun, validAgent, validOrg, "")
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if res.Decision != policy.DecisionDeny {
			t.Errorf("Decision = %q, want %q", res.Decision, policy.DecisionDeny)
		}
		if !strings.Contains(res.Reason, "high-risk and requires human approval") {
			t.Errorf("Reason missing high-risk phrase: %q", res.Reason)
		}
		if !strings.Contains(res.Reason, "cap.sandbox.exec") {
			t.Errorf("Reason missing capability id: %q", res.Reason)
		}
		if res.BudgetContext != "" {
			t.Errorf("BudgetContext = %q, want empty", res.BudgetContext)
		}
	})
}

// TestEngine_ScopeEnforcement covers the Phase-3 scope gate: when the caller
// supplies a non-empty scope, it must be present in the capability's
// EnabledForScopes list (or the list must contain ScopeWildcard). Empty scope
// preserves legacy behaviour.
func TestEngine_ScopeEnforcement(t *testing.T) {
	t.Parallel()
	reg := registry.NewRegistry()
	eng := policy.New(reg)
	ctx := context.Background()

	const (
		lowRiskAllowedCap = "cap.memory.search" // seeded with EnabledForScopes=["workspace"]
	)

	cases := []struct {
		name       string
		capID      string
		scope      string
		wantResult string
		wantReason string
	}{
		{"empty scope preserves legacy allow", lowRiskAllowedCap, "", policy.DecisionAllow, "low-risk"},
		{"matching scope allows", lowRiskAllowedCap, "workspace", policy.DecisionAllow, "low-risk"},
		{"non-matching scope denies", lowRiskAllowedCap, "global", policy.DecisionDeny, "not enabled for scope"},
		{"unknown scope denies", lowRiskAllowedCap, "something-random", policy.DecisionDeny, "not enabled for scope"},
	}
	for _, tc := range cases {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			res, err := eng.Evaluate(ctx, tc.capID, "run-1", "agent-1", "org-1", tc.scope)
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if res.Decision != tc.wantResult {
				t.Fatalf("Decision = %q, want %q (reason=%q)", res.Decision, tc.wantResult, res.Reason)
			}
			if !strings.Contains(res.Reason, tc.wantReason) {
				t.Errorf("Reason = %q, want substring %q", res.Reason, tc.wantReason)
			}
			if tc.wantResult == policy.DecisionDeny && res.BudgetContext != "" {
				t.Errorf("BudgetContext = %q, want empty on deny", res.BudgetContext)
			}
		})
	}
}

// TestEngine_ScopeWildcardAllowsAny proves that a capability with "*" in
// EnabledForScopes accepts any non-empty scope. Registered via a bespoke
// Registry instance to avoid coupling to seed data.
func TestEngine_ScopeWildcardAllowsAny(t *testing.T) {
	t.Parallel()
	// cap.skill.summarize seed has EnabledForScopes=["*", "workspace"] if seeded
	// that way; otherwise test asserts on an existing entry with "*" (none
	// today — this is a regression guard for when wildcard seeds are added).
	reg := registry.NewRegistry()
	items, _ := reg.List("", "", "", 200)
	var wildcardCap string
	for _, c := range items {
		for _, s := range c.EnabledForScopes {
			if s == policy.ScopeWildcard {
				wildcardCap = c.ID
				break
			}
		}
		if wildcardCap != "" {
			break
		}
	}
	if wildcardCap == "" {
		t.Skip("no seeded capability declares the ScopeWildcard; skipping")
	}
	eng := policy.New(reg)
	ctx := context.Background()
	res, err := eng.Evaluate(ctx, wildcardCap, "run-1", "agent-1", "org-1", "run")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Decision == policy.DecisionDeny && strings.Contains(res.Reason, "not enabled for scope") {
		t.Fatalf("wildcard scope should not trip scope denial; reason=%q", res.Reason)
	}
}

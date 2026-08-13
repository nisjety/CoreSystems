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

// TestEngine_EvaluateCapability_ArgumentValidation covers the argument
// guards EvaluateCapability enforces on behalf of every caller (a resolved
// capability, run, agent, org, and a supported scope are all mandatory). This
// used to be exercised through the now-removed Evaluate(capID, ...)
// convenience wrapper, which resolved capID via the registry's org-blind,
// last-write-wins Get — a landmine closed by POL-1's sibling finding POL-2.
// Callers must resolve a capability through a tenant-scoped lookup
// (registry.GetForOrg or the durable store's GetForOrg) themselves and call
// EvaluateCapability directly, as every real gRPC/HTTP entry point already does.
func TestEngine_EvaluateCapability_ArgumentValidation(t *testing.T) {
	reg := registry.NewRegistry()
	eng := policy.New(reg)
	ctx := context.Background()

	const (
		validRun   = "run-1"
		validAgent = "agent-1"
		validOrg   = "org-1"
	)
	// The static seed's OrgID is the fixed "triodelab" owner (see
	// staticSeedSource.Load in registry.go); GetForOrg is the tenant-safe
	// resolution path that replaced the org-blind Get.
	validCap, err := reg.GetForOrg("cap.memory.search", "", "triodelab")
	if err != nil {
		t.Fatalf("resolve cap.memory.search: %v", err)
	}

	t.Run("nil capability returns ErrInvalidArgument", func(t *testing.T) {
		_, err := eng.EvaluateCapability(ctx, nil, validRun, validAgent, validOrg, "global")
		if !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("expected ErrInvalidArgument, got %v", err)
		}
	})

	t.Run("empty runID returns ErrInvalidArgument", func(t *testing.T) {
		_, err := eng.EvaluateCapability(ctx, validCap, "", validAgent, validOrg, "global")
		if !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("expected ErrInvalidArgument, got %v", err)
		}
	})

	t.Run("empty agentID returns ErrInvalidArgument", func(t *testing.T) {
		_, err := eng.EvaluateCapability(ctx, validCap, validRun, "", validOrg, "global")
		if !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("expected ErrInvalidArgument, got %v", err)
		}
	})

	t.Run("empty orgID returns ErrInvalidArgument", func(t *testing.T) {
		_, err := eng.EvaluateCapability(ctx, validCap, validRun, validAgent, "", "global")
		if !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("expected ErrInvalidArgument, got %v", err)
		}
	})

	t.Run("low-risk capability is allowed with default budget", func(t *testing.T) {
		lowRisk, err := reg.GetForOrg("cap.policy.round-robin", "", "triodelab")
		if err != nil {
			t.Fatal(err)
		}
		res, err := eng.EvaluateCapability(ctx, lowRisk, validRun, validAgent, validOrg, "global")
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
		capability, err := reg.GetForOrg("cap.browser.open", "", "triodelab")
		if err != nil {
			t.Fatal(err)
		}
		authorized := *capability
		authorized.Scope = "global"
		authorized.EnabledForScopes = []string{"global"}
		res, err := eng.EvaluateCapability(ctx, &authorized, validRun, validAgent, validOrg, "global")
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

	t.Run("high-risk capability requires durable approval with empty budget", func(t *testing.T) {
		capability, err := reg.GetForOrg("cap.sandbox.exec", "", "triodelab")
		if err != nil {
			t.Fatal(err)
		}
		authorized := *capability
		authorized.Scope = "global"
		authorized.EnabledForScopes = []string{"global"}
		res, err := eng.EvaluateCapability(ctx, &authorized, validRun, validAgent, validOrg, "global")
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if res.Decision != policy.DecisionAsk {
			t.Errorf("Decision = %q, want %q", res.Decision, policy.DecisionAsk)
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

func TestEngineEvaluateCapabilityUsesAuthorizedSnapshot(t *testing.T) {
	t.Parallel()

	reg := registry.NewRegistry()
	engine := policy.New(reg)
	authorized, err := reg.GetForOrg("cap.policy.round-robin", "", "triodelab")
	if err != nil {
		t.Fatal(err)
	}

	result, err := engine.EvaluateCapability(
		context.Background(), authorized, "run-1", "agent-1", "triodelab", "global",
	)
	if err != nil {
		t.Fatalf("EvaluateCapability: %v", err)
	}
	if result.Decision != policy.DecisionAllow {
		t.Fatalf("decision = %+v", result)
	}

	if _, err := engine.EvaluateCapability(context.Background(), nil, "run-1", "agent-1", "triodelab", ""); !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("nil authorized capability error = %v", err)
	}
}

// TestEngine_ScopeEnforcement covers the Phase-3 scope gate: when the caller
// supplies a canonical scope, it must be present in the capability's
// EnabledForScopes list (or the list must contain ScopeWildcard).
func TestEngine_ScopeEnforcement(t *testing.T) {
	t.Parallel()
	reg := registry.NewRegistry()
	eng := policy.New(reg)
	ctx := context.Background()

	// seeded with EnabledForScopes=["global"]; static seed OrgID is "triodelab".
	capability, err := reg.GetForOrg("cap.policy.round-robin", "", "triodelab")
	if err != nil {
		t.Fatalf("resolve cap.policy.round-robin: %v", err)
	}

	cases := []struct {
		name       string
		scope      string
		wantResult string
		wantReason string
	}{
		{"matching scope allows", "global", policy.DecisionAllow, "low-risk"},
		{"non-matching scope denies", "org", policy.DecisionDeny, "not enabled for scope"},
	}
	for _, tc := range cases {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			res, err := eng.EvaluateCapability(ctx, capability, "run-1", "agent-1", "org-1", tc.scope)
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

// TestEngine_ScopeWildcardAllowsSupportedScope proves that a capability with
// "*" in EnabledForScopes accepts a supported invocation scope. Registered via a bespoke
// Registry instance to avoid coupling to seed data.
func TestEngine_ScopeWildcardAllowsSupportedScope(t *testing.T) {
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
	capability, err := reg.GetForOrg(wildcardCap, "", "triodelab")
	if err != nil {
		t.Fatalf("resolve %s: %v", wildcardCap, err)
	}
	res, err := eng.EvaluateCapability(ctx, capability, "run-1", "agent-1", "org-1", "global")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Decision == policy.DecisionDeny && strings.Contains(res.Reason, "not enabled for scope") {
		t.Fatalf("wildcard scope should not trip scope denial; reason=%q", res.Reason)
	}
}

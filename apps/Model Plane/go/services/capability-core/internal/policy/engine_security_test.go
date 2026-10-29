package policy_test

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

func TestEngineRejectsMissingAndUnsupportedInvocationScopes(t *testing.T) {
	t.Parallel()
	reg := registry.NewRegistry()
	engine := policy.New(reg)
	capability, err := reg.GetForOrg("cap.memory.search", "", "triodelab")
	if err != nil {
		t.Fatalf("resolve cap.memory.search: %v", err)
	}

	for _, scope := range []string{"", "something-random"} {
		_, err := engine.EvaluateCapability(
			context.Background(), capability, "run-1", "agent-1", "triodelab", scope,
		)
		if !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("scope %q error = %v, want ErrInvalidArgument", scope, err)
		}
	}
}

func TestEngineRejectsScopesWithoutTrustedConcreteIDsBeforeGrantEvaluation(t *testing.T) {
	t.Parallel()
	capability := &models.Capability{
		ID: "cap.unsupported-scope", Name: "Unsupported scope", Kind: models.KindTool,
		Version: "1", RiskLevel: models.RiskLow, Enabled: true,
		EnabledForScopes: []string{policy.ScopeWildcard},
	}
	resolver := &fakeResolver{grants: map[string]map[string]map[string]map[string]bool{}}
	engine := policy.New(registry.NewRegistry()).WithScopeResolver(resolver)

	for _, scope := range []string{
		registry.ScopeKindRun,
		registry.ScopeKindThread,
		registry.ScopeKindWorkspace,
		registry.ScopeKindUser,
		registry.ScopeKindAgent,
	} {
		_, err := engine.EvaluateCapability(
			context.Background(), capability, "run-1", "agent-1", "triodelab", scope,
		)
		if !errors.Is(err, domain.ErrInvalidArgument) {
			t.Fatalf("scope %q error = %v, want ErrInvalidArgument", scope, err)
		}
	}
	if resolver.calls != 0 {
		t.Fatalf("unsupported scopes reached durable grant lookup %d times", resolver.calls)
	}
}

func TestEngineSupportsOnlyScopesWithTrustedAuthorityValues(t *testing.T) {
	t.Parallel()
	for _, scope := range []string{
		registry.ScopeKindGlobal,
		registry.ScopeKindOrg,
	} {
		if !policy.IsSupportedScope(scope) {
			t.Fatalf("scope %q should remain supported", scope)
		}
	}
	for _, scope := range []string{
		registry.ScopeKindRun,
		registry.ScopeKindThread,
		registry.ScopeKindWorkspace,
		registry.ScopeKindUser,
		registry.ScopeKindAgent,
	} {
		if policy.IsSupportedScope(scope) {
			t.Fatalf("scope %q must fail closed until trusted concrete IDs exist", scope)
		}
	}
}

func TestEngineDeniesCapabilityWithoutStaticScopeAuthorization(t *testing.T) {
	t.Parallel()
	capability := &models.Capability{
		ID: "cap.no-scopes", Name: "No scopes", Kind: models.KindTool,
		Version: "1", RiskLevel: models.RiskLow, Enabled: true,
	}
	engine := policy.New(registry.NewRegistry())

	result, err := engine.EvaluateCapability(
		context.Background(), capability, "run-1", "agent-1", "triodelab", "global",
	)
	if err != nil {
		t.Fatalf("EvaluateCapability: %v", err)
	}
	if result.Decision != policy.DecisionDeny || !strings.Contains(result.Reason, "not enabled") {
		t.Fatalf("decision = %+v, want fail-closed scope denial", result)
	}
}

func TestEngineDeniesUnknownRiskLevel(t *testing.T) {
	t.Parallel()
	capability := &models.Capability{
		ID: "cap.unknown-risk", Name: "Unknown risk", Kind: models.KindTool,
		Version: "1", RiskLevel: "critical-ish", Enabled: true,
		EnabledForScopes: []string{"global"},
	}
	engine := policy.New(registry.NewRegistry())

	result, err := engine.EvaluateCapability(
		context.Background(), capability, "run-1", "agent-1", "triodelab", "global",
	)
	if err != nil {
		t.Fatalf("EvaluateCapability: %v", err)
	}
	if result.Decision != policy.DecisionDeny || result.Reason != "invalid_risk_level" || result.BudgetContext != "" {
		t.Fatalf("decision = %+v, want invalid-risk denial", result)
	}
}

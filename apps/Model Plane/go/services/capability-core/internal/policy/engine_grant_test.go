package policy_test

import (
	"context"
	"errors"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// fakeResolver is an in-memory ScopeResolver: grants[capID][kind] holds the set
// of granted scope_values ("*" = wildcard).
type fakeResolver struct {
	grants map[string]map[string]map[string]map[string]bool
	err    error
	calls  int
}

func (f *fakeResolver) HasAnyGrants(_ context.Context, capID, orgID, kind string) (bool, error) {
	if f.err != nil {
		return false, f.err
	}
	return len(f.grants[orgID][capID][kind]) > 0, nil
}

func (f *fakeResolver) IsGrantedForScope(_ context.Context, capID, orgID, kind, value string) (bool, error) {
	f.calls++
	if f.err != nil {
		return false, f.err
	}
	vals := f.grants[orgID][capID][kind]
	return vals[value] || vals["*"], nil
}

func grantResolver(orgID, capID, kind string, values ...string) *fakeResolver {
	r := &fakeResolver{grants: map[string]map[string]map[string]map[string]bool{
		orgID: {capID: {kind: {}}},
	}}
	for _, v := range values {
		r.grants[orgID][capID][kind][v] = true
	}
	return r
}

// orgScopedCapability returns a low-risk capability that supports the "org"
// scope, so the static EnabledForScopes check passes and the durable grant
// check governs. It is a plain struct rather than something resolved through
// a Registry: these tests exercise EvaluateCapability's grant/scope decision
// logic directly, so there is no need to route through a lookup at all (the
// removed Evaluate(capID, ...) wrapper used to do that resolution through the
// registry's org-blind Get — see POL-2).
func orgScopedCapability() (*models.Capability, string) {
	const capID = "cap.test.org-scoped"
	return &models.Capability{
		ID: capID, Name: "Org Scoped Tool", Kind: models.KindTool, Version: "1.0.0",
		RiskLevel: models.RiskLow, Scope: "org", Enabled: true,
		EnabledForScopes: []string{"org"},
	}, capID
}

func TestEngine_DurableGrant_AllowsWhenGranted(t *testing.T) {
	capability, capID := orgScopedCapability()
	eng := policy.New(nil).WithScopeResolver(grantResolver("org-allowed", capID, registry.ScopeKindOrg, "org-allowed"))

	res, err := eng.EvaluateCapability(context.Background(), capability, "run-1", "agent-1", "org-allowed", "org")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Decision != policy.DecisionAllow {
		t.Fatalf("expected allow for granted org, got %q (%s)", res.Decision, res.Reason)
	}
}

func TestEngine_DurableGrant_DeniesWhenNotGranted(t *testing.T) {
	capability, capID := orgScopedCapability()
	// Capability is governed by org grants, but only for a different org.
	eng := policy.New(nil).WithScopeResolver(grantResolver("org-allowed", capID, registry.ScopeKindOrg, "org-other"))

	res, err := eng.EvaluateCapability(context.Background(), capability, "run-1", "agent-1", "org-allowed", "org")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Decision != policy.DecisionDeny {
		t.Fatalf("expected deny for ungranted org, got %q", res.Decision)
	}
}

func TestEngine_DurableGrant_AgentWildcardCannotReviveUnsupportedScope(t *testing.T) {
	const capID = "cap.test.agent-wildcard"
	capability := &models.Capability{
		ID: capID, Name: "Agent wildcard", Kind: models.KindSkill, Version: "1.0.0",
		RiskLevel: models.RiskLow, Scope: "agent", Enabled: true,
		EnabledForScopes: []string{"agent"},
	}
	resolver := grantResolver("org-a", capID, registry.ScopeKindAgent, "*")
	eng := policy.New(nil).WithScopeResolver(resolver)

	_, err := eng.EvaluateCapability(context.Background(), capability, "run-1", "agent-1", "org-a", "agent")
	if !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("agent wildcard error = %v, want ErrInvalidArgument", err)
	}
	if resolver.calls != 0 {
		t.Fatalf("unsupported agent scope reached resolver")
	}
}

func TestEngine_DurableGrant_MissingGrantFailsClosed(t *testing.T) {
	capability, _ := orgScopedCapability()
	// Resolver has no grants for this tenant and capability.
	eng := policy.New(nil).WithScopeResolver(&fakeResolver{grants: map[string]map[string]map[string]map[string]bool{}})

	res, err := eng.EvaluateCapability(context.Background(), capability, "run-1", "agent-1", "org-allowed", "org")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Decision != policy.DecisionDeny {
		t.Fatalf("expected deny without durable grant, got %q", res.Decision)
	}
}

func TestEngine_DurableGrant_ExactAgentGrantCannotReviveUnsupportedScope(t *testing.T) {
	const capID = "cap.test.agent-scoped"
	capability := &models.Capability{
		ID: capID, Name: "Agent Scoped", Kind: models.KindSkill, Version: "1.0.0",
		RiskLevel: models.RiskLow, Scope: "agent", Enabled: true,
		EnabledForScopes: []string{"agent"},
	}
	resolver := grantResolver("org-1", capID, registry.ScopeKindAgent, "agent-7")
	eng := policy.New(nil).WithScopeResolver(resolver)

	_, err := eng.EvaluateCapability(context.Background(), capability, "run-1", "agent-7", "org-1", "agent")
	if !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("exact agent grant error = %v, want ErrInvalidArgument", err)
	}
	if resolver.calls != 0 {
		t.Fatalf("unsupported agent scope reached resolver")
	}
}

func TestEngine_DurableAgentGrantDoesNotCrossTenantOnCollidingAgentID(t *testing.T) {
	const capID = "cap.test.agent-tenant"
	capability := &models.Capability{
		ID: capID, Name: "Tenant-bound agent", Kind: models.KindSkill, Version: "1.0.0",
		RiskLevel: models.RiskLow, Scope: "agent", Enabled: true,
		EnabledForScopes: []string{"agent"},
	}
	engine := policy.New(nil).WithScopeResolver(
		grantResolver("org-a", capID, registry.ScopeKindAgent, "shared-agent-id"),
	)

	_, err := engine.EvaluateCapability(
		context.Background(), capability, "run-1", "shared-agent-id", "org-b", "agent",
	)
	if !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("cross-tenant colliding agent error = %v, want ErrInvalidArgument", err)
	}
}

func TestEngine_DurableGrant_ResolverErrorSurfaces(t *testing.T) {
	capability, capID := orgScopedCapability()
	boom := errors.New("db down")
	eng := policy.New(nil).WithScopeResolver(&fakeResolver{
		grants: map[string]map[string]map[string]map[string]bool{
			"org-allowed": {capID: {registry.ScopeKindOrg: {"org-allowed": true}}},
		},
		err: boom,
	})
	_, err := eng.EvaluateCapability(context.Background(), capability, "run-1", "agent-1", "org-allowed", "org")
	if !errors.Is(err, boom) {
		t.Fatalf("expected resolver error to surface, got %v", err)
	}
}

func TestEngine_DurableGrant_NilResolverPreservesStaticBehavior(t *testing.T) {
	capability, _ := orgScopedCapability()
	eng := policy.New(nil) // no resolver
	res, err := eng.EvaluateCapability(context.Background(), capability, "run-1", "agent-1", "org-allowed", "org")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Decision != policy.DecisionAllow {
		t.Fatalf("expected allow (static EnabledForScopes contains org), got %q", res.Decision)
	}
}

func TestEngine_GlobalScopePreservesStaticCatalogSemantics(t *testing.T) {
	const capID = "cap.test.global"
	capability := &models.Capability{
		ID: capID, Name: "Global", Kind: models.KindTool, Version: "1.0.0",
		RiskLevel: models.RiskLow, Scope: registry.ScopeKindGlobal, Enabled: true,
		EnabledForScopes: []string{registry.ScopeKindGlobal},
	}
	resolver := &fakeResolver{
		grants: map[string]map[string]map[string]map[string]bool{},
		err:    errors.New("global scope must not require a concrete grant value"),
	}
	engine := policy.New(nil).WithScopeResolver(resolver)

	result, err := engine.EvaluateCapability(
		context.Background(), capability, "run-1", "agent-1", "org-1", registry.ScopeKindGlobal,
	)
	if err != nil {
		t.Fatalf("global evaluation: %v", err)
	}
	if result.Decision != policy.DecisionAllow {
		t.Fatalf("global decision = %+v, want static allow", result)
	}
	if resolver.calls != 0 {
		t.Fatalf("global scope unexpectedly required concrete grant lookup")
	}
}

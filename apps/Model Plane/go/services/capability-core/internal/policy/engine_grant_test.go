package policy_test

import (
	"context"
	"errors"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// fakeSource lets the test seed an arbitrary capability set into a Registry.
type fakeSource struct{ caps []*models.Capability }

func (f fakeSource) Load() ([]*models.Capability, error) { return f.caps, nil }

// fakeResolver is an in-memory ScopeResolver: grants[capID][kind] holds the set
// of granted scope_values ("*" = wildcard).
type fakeResolver struct {
	grants map[string]map[string]map[string]bool
	err    error
}

func (f *fakeResolver) HasAnyGrants(_ context.Context, capID, kind string) (bool, error) {
	if f.err != nil {
		return false, f.err
	}
	return len(f.grants[capID][kind]) > 0, nil
}

func (f *fakeResolver) IsGrantedForScope(_ context.Context, capID, kind, value string) (bool, error) {
	if f.err != nil {
		return false, f.err
	}
	vals := f.grants[capID][kind]
	return vals[value] || vals["*"], nil
}

func grantResolver(capID, kind string, values ...string) *fakeResolver {
	r := &fakeResolver{grants: map[string]map[string]map[string]bool{capID: {kind: {}}}}
	for _, v := range values {
		r.grants[capID][kind][v] = true
	}
	return r
}

// orgScopedReg seeds a low-risk capability that supports the "org" scope so the
// static EnabledForScopes check passes and the durable grant check governs.
func orgScopedReg(t *testing.T) (*registry.Registry, string) {
	t.Helper()
	const capID = "cap.test.org-scoped"
	reg, err := registry.NewFromSource(fakeSource{caps: []*models.Capability{{
		ID: capID, Name: "Org Scoped Tool", Kind: models.KindTool, Version: "1.0.0",
		RiskLevel: models.RiskLow, Scope: "org", Enabled: true,
		EnabledForScopes: []string{"org"},
	}}})
	if err != nil {
		t.Fatalf("build registry: %v", err)
	}
	return reg, capID
}

func TestEngine_DurableGrant_AllowsWhenGranted(t *testing.T) {
	reg, capID := orgScopedReg(t)
	eng := policy.New(reg).WithScopeResolver(grantResolver(capID, registry.ScopeKindOrg, "org-allowed"))

	res, err := eng.Evaluate(context.Background(), capID, "run-1", "agent-1", "org-allowed", "org")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Decision != policy.DecisionAllow {
		t.Fatalf("expected allow for granted org, got %q (%s)", res.Decision, res.Reason)
	}
}

func TestEngine_DurableGrant_DeniesWhenNotGranted(t *testing.T) {
	reg, capID := orgScopedReg(t)
	// Capability is governed by org grants, but only for a different org.
	eng := policy.New(reg).WithScopeResolver(grantResolver(capID, registry.ScopeKindOrg, "org-other"))

	res, err := eng.Evaluate(context.Background(), capID, "run-1", "agent-1", "org-allowed", "org")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Decision != policy.DecisionDeny {
		t.Fatalf("expected deny for ungranted org, got %q", res.Decision)
	}
}

func TestEngine_DurableGrant_WildcardCoversAnyValue(t *testing.T) {
	reg, capID := orgScopedReg(t)
	eng := policy.New(reg).WithScopeResolver(grantResolver(capID, registry.ScopeKindOrg, "*"))

	res, err := eng.Evaluate(context.Background(), capID, "run-1", "agent-1", "any-org-at-all", "org")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Decision != policy.DecisionAllow {
		t.Fatalf("expected allow under wildcard grant, got %q (%s)", res.Decision, res.Reason)
	}
}

func TestEngine_DurableGrant_UngovernedCapabilityFallsThrough(t *testing.T) {
	reg, capID := orgScopedReg(t)
	// Resolver has NO grants for this capability of kind org → not governed →
	// engine defers to the static EnabledForScopes (which includes "org") → allow.
	eng := policy.New(reg).WithScopeResolver(&fakeResolver{grants: map[string]map[string]map[string]bool{}})

	res, err := eng.Evaluate(context.Background(), capID, "run-1", "agent-1", "org-allowed", "org")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Decision != policy.DecisionAllow {
		t.Fatalf("expected allow for ungoverned capability, got %q", res.Decision)
	}
}

func TestEngine_DurableGrant_AgentScope(t *testing.T) {
	const capID = "cap.test.agent-scoped"
	reg, err := registry.NewFromSource(fakeSource{caps: []*models.Capability{{
		ID: capID, Name: "Agent Scoped", Kind: models.KindSkill, Version: "1.0.0",
		RiskLevel: models.RiskLow, Scope: "agent", Enabled: true,
		EnabledForScopes: []string{"agent"},
	}}})
	if err != nil {
		t.Fatalf("build registry: %v", err)
	}
	eng := policy.New(reg).WithScopeResolver(grantResolver(capID, registry.ScopeKindAgent, "agent-7"))

	allowed, err := eng.Evaluate(context.Background(), capID, "run-1", "agent-7", "org-1", "agent")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if allowed.Decision != policy.DecisionAllow {
		t.Fatalf("expected allow for granted agent, got %q", allowed.Decision)
	}
	denied, err := eng.Evaluate(context.Background(), capID, "run-1", "agent-9", "org-1", "agent")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if denied.Decision != policy.DecisionDeny {
		t.Fatalf("expected deny for ungranted agent, got %q", denied.Decision)
	}
}

func TestEngine_DurableGrant_ResolverErrorSurfaces(t *testing.T) {
	reg, capID := orgScopedReg(t)
	boom := errors.New("db down")
	eng := policy.New(reg).WithScopeResolver(&fakeResolver{
		grants: map[string]map[string]map[string]bool{capID: {registry.ScopeKindOrg: {"org-allowed": true}}},
		err:    boom,
	})
	_, err := eng.Evaluate(context.Background(), capID, "run-1", "agent-1", "org-allowed", "org")
	if !errors.Is(err, boom) {
		t.Fatalf("expected resolver error to surface, got %v", err)
	}
}

func TestEngine_DurableGrant_NilResolverPreservesStaticBehavior(t *testing.T) {
	reg, capID := orgScopedReg(t)
	eng := policy.New(reg) // no resolver
	res, err := eng.Evaluate(context.Background(), capID, "run-1", "agent-1", "org-allowed", "org")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Decision != policy.DecisionAllow {
		t.Fatalf("expected allow (static EnabledForScopes contains org), got %q", res.Decision)
	}
}

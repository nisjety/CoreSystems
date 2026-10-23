package policy

import (
	"context"
	"errors"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

const (
	digestA = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	digestB = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
)

func mcpCapability(risk string) *models.Capability {
	return &models.Capability{ID: "cap.mcp_tool.x", Kind: models.KindMCPTool, Version: "1.0.0", RiskLevel: risk, EnabledForScopes: []string{ScopeWildcard}}
}

func builtinCapability(risk string) *models.Capability {
	return &models.Capability{ID: "cap.web.fetch", Kind: models.KindTool, Version: "1.0.0", RiskLevel: risk, EnabledForScopes: []string{ScopeWildcard}}
}

// mcpBinding is an approved MCP tool row pinned to digestA.
func mcpBinding(class string) *ToolBinding {
	return &ToolBinding{
		ToolName: "mcp__s__t", CapabilityID: "cap.mcp_tool.x", SideEffectClass: class,
		ApprovalFloor: models.MinimumApprovalFloor(class), ToolDigest: digestA, ReviewState: ReviewStateApproved,
	}
}

func askBase() *Result {
	return &Result{Decision: DecisionAsk, Reason: "capability is high-risk and requires human approval", PolicySource: PolicySourceRiskLevel}
}

func policyOf(scope, decision, digest string) ToolPolicy {
	return ToolPolicy{ScopeKind: scope, Decision: decision, PinnedDigest: digest}
}

func TestDecideWithPoliciesRelaxAndTighten(t *testing.T) {
	user, org := ToolPolicyScopeUser, ToolPolicyScopeOrg
	tests := []struct {
		name       string
		capability *models.Capability
		base       *Result
		binding    *ToolBinding
		policies   []ToolPolicy
		want       string
		wantSource string
	}{
		{
			name:       "no policies is exactly Decide",
			capability: mcpCapability(models.RiskHigh), base: askBase(), binding: mcpBinding(models.SideEffectExternalWrite),
			want: DecisionAsk, wantSource: PolicySourceRiskLevel,
		},
		{
			name:       "a user allow on a pinned external write turns the risk ask into an allow",
			capability: mcpCapability(models.RiskHigh), base: askBase(), binding: mcpBinding(models.SideEffectExternalWrite),
			policies: []ToolPolicy{policyOf(user, ToolPolicyAllow, digestA)},
			want:     DecisionAllow, wantSource: PolicySourceToolPolicyAllow,
		},
		{
			name:       "an org ask beats a user allow",
			capability: mcpCapability(models.RiskHigh), base: askBase(), binding: mcpBinding(models.SideEffectExternalWrite),
			policies: []ToolPolicy{policyOf(org, ToolPolicyAsk, ""), policyOf(user, ToolPolicyAllow, digestA)},
			want:     DecisionAsk, wantSource: PolicySourceRiskLevel,
		},
		{
			name:       "an org never beats a user allow",
			capability: mcpCapability(models.RiskHigh), base: askBase(), binding: mcpBinding(models.SideEffectExternalWrite),
			policies: []ToolPolicy{policyOf(org, ToolPolicyNever, ""), policyOf(user, ToolPolicyAllow, digestA)},
			want:     DecisionDeny, wantSource: PolicySourceToolPolicyNever,
		},
		{
			name:       "a user never beats an org allow",
			capability: mcpCapability(models.RiskHigh), base: askBase(), binding: mcpBinding(models.SideEffectExternalWrite),
			policies: []ToolPolicy{policyOf(org, ToolPolicyAllow, digestA), policyOf(user, ToolPolicyNever, "")},
			want:     DecisionDeny, wantSource: PolicySourceToolPolicyNever,
		},
		{
			name:       "a user ask stands under an org allow",
			capability: mcpCapability(models.RiskHigh), base: askBase(), binding: mcpBinding(models.SideEffectExternalWrite),
			policies: []ToolPolicy{policyOf(org, ToolPolicyAllow, digestA), policyOf(user, ToolPolicyAsk, "")},
			want:     DecisionAsk, wantSource: PolicySourceRiskLevel,
		},
		{
			name:       "a changed definition demotes an allow to an ask",
			capability: mcpCapability(models.RiskLow), base: allowBase(), binding: func() *ToolBinding { b := mcpBinding(models.SideEffectRead); b.ToolDigest = digestB; return b }(),
			policies: []ToolPolicy{policyOf(user, ToolPolicyAllow, digestA)},
			want:     DecisionAsk, wantSource: PolicySourceToolPolicyAsk,
		},
		{
			name:       "a changed definition leaves a risk ask an ask",
			capability: mcpCapability(models.RiskHigh), base: askBase(), binding: func() *ToolBinding { b := mcpBinding(models.SideEffectExternalWrite); b.ToolDigest = digestB; return b }(),
			policies: []ToolPolicy{policyOf(user, ToolPolicyAllow, digestA)},
			want:     DecisionAsk, wantSource: PolicySourceRiskLevel,
		},
		{
			name:       "an allow with no digest relaxes nothing",
			capability: mcpCapability(models.RiskHigh), base: askBase(), binding: mcpBinding(models.SideEffectExternalWrite),
			policies: []ToolPolicy{policyOf(user, ToolPolicyAllow, "")},
			want:     DecisionAsk, wantSource: PolicySourceRiskLevel,
		},
		{
			name:       "a built-in tool's allow is ignored",
			capability: builtinCapability(models.RiskHigh), base: askBase(),
			binding:  &ToolBinding{ToolName: "shell", CapabilityID: "cap.web.fetch", SideEffectClass: models.SideEffectExternalWrite, ApprovalFloor: models.ApprovalFloorNone},
			policies: []ToolPolicy{policyOf(user, ToolPolicyAllow, digestA), policyOf(org, ToolPolicyAllow, digestA)},
			want:     DecisionAsk, wantSource: PolicySourceRiskLevel,
		},
		{
			name:       "an owner effect is never relaxed, even by the org",
			capability: mcpCapability(models.RiskHigh), base: askBase(),
			binding: func() *ToolBinding {
				b := mcpBinding(models.SideEffectOwnerEffect)
				b.ApprovalFloor = models.ApprovalFloorDurableOwner
				return b
			}(),
			policies: []ToolPolicy{policyOf(org, ToolPolicyAllow, digestA)},
			want:     DecisionAsk, wantSource: PolicySourceRiskLevel,
		},
		{
			name:       "an unclassified tool is never relaxed",
			capability: mcpCapability(models.RiskHigh), base: askBase(), binding: mcpBinding(models.SideEffectUnclassified),
			policies: []ToolPolicy{policyOf(org, ToolPolicyAllow, digestA)},
			want:     DecisionAsk, wantSource: PolicySourceRiskLevel,
		},
		{
			name:       "a tool with an ask floor is not relaxed",
			capability: mcpCapability(models.RiskHigh), base: askBase(),
			binding: func() *ToolBinding {
				b := mcpBinding(models.SideEffectExternalWrite)
				b.ApprovalFloor = models.ApprovalFloorAsk
				return b
			}(),
			policies: []ToolPolicy{policyOf(user, ToolPolicyAllow, digestA)},
			want:     DecisionAsk, wantSource: PolicySourceRiskLevel,
		},
		{
			name:       "a user allow cannot relax a destructive tool",
			capability: mcpCapability(models.RiskHigh), base: askBase(), binding: mcpBinding(models.SideEffectDestructive),
			policies: []ToolPolicy{policyOf(user, ToolPolicyAllow, digestA)},
			want:     DecisionAsk, wantSource: PolicySourceRiskLevel,
		},
		{
			name:       "an org allow can relax a destructive tool",
			capability: mcpCapability(models.RiskHigh), base: askBase(), binding: mcpBinding(models.SideEffectDestructive),
			policies: []ToolPolicy{policyOf(org, ToolPolicyAllow, digestA)},
			want:     DecisionAllow, wantSource: PolicySourceToolPolicyAllow,
		},
		{
			name:       "an allow has no binding to relax and a tool-less evaluation is unchanged",
			capability: mcpCapability(models.RiskHigh), base: askBase(),
			policies: []ToolPolicy{policyOf(user, ToolPolicyAllow, digestA)},
			want:     DecisionAsk, wantSource: PolicySourceRiskLevel,
		},
		{
			name:       "an ask turns a low-risk allow into an ask",
			capability: builtinCapability(models.RiskLow), base: allowBase(),
			binding:  &ToolBinding{ToolName: "web_fetch", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone},
			policies: []ToolPolicy{policyOf(user, ToolPolicyAsk, "")},
			want:     DecisionAsk, wantSource: PolicySourceToolPolicyAsk,
		},
		{
			name:       "a never denies a built-in tool",
			capability: builtinCapability(models.RiskLow), base: allowBase(),
			binding:  &ToolBinding{ToolName: "web_fetch", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone},
			policies: []ToolPolicy{policyOf(org, ToolPolicyNever, "")},
			want:     DecisionDeny, wantSource: PolicySourceToolPolicyNever,
		},
		{
			name:       "a never applies to a tool-less evaluation too",
			capability: builtinCapability(models.RiskLow), base: allowBase(),
			policies: []ToolPolicy{policyOf(user, ToolPolicyNever, "")},
			want:     DecisionDeny, wantSource: PolicySourceToolPolicyNever,
		},
		{
			name:       "a decision outside the vocabulary only adds a pause",
			capability: builtinCapability(models.RiskLow), base: allowBase(),
			policies: []ToolPolicy{policyOf(user, "maybe", "")},
			want:     DecisionAsk, wantSource: PolicySourceToolPolicyAsk,
		},
		{
			name:       "a tool that is not approved stays denied under an allow",
			capability: mcpCapability(models.RiskHigh), base: askBase(),
			binding: func() *ToolBinding {
				b := mcpBinding(models.SideEffectExternalWrite)
				b.ReviewState = ReviewStateDrifted
				return b
			}(),
			policies: []ToolPolicy{policyOf(user, ToolPolicyAllow, digestA)},
			want:     DecisionDeny, wantSource: PolicySourceToolReview,
		},
		{
			name:       "a capability-gate denial stands under an allow",
			capability: mcpCapability(models.RiskHigh), binding: mcpBinding(models.SideEffectExternalWrite),
			base:     &Result{Decision: DecisionDeny, Reason: "rollout_quarantine", PolicySource: PolicySourceCapabilityGate},
			policies: []ToolPolicy{policyOf(user, ToolPolicyAllow, digestA)},
			want:     DecisionDeny, wantSource: PolicySourceCapabilityGate,
		},
		{
			name:       "an ask that came from the approval floor is not relaxed by an allow",
			capability: mcpCapability(models.RiskLow), base: allowBase(),
			binding: func() *ToolBinding {
				b := mcpBinding(models.SideEffectExternalWrite)
				b.ApprovalFloor = models.ApprovalFloorAsk
				return b
			}(),
			policies: []ToolPolicy{policyOf(org, ToolPolicyAllow, digestA)},
			want:     DecisionAsk, wantSource: PolicySourceApprovalFloor,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := DecideWithPolicies(tt.base, tt.capability, tt.binding, tt.policies)
			if got.Decision != tt.want || got.PolicySource != tt.wantSource {
				t.Fatalf("got %s/%s (%s), want %s/%s", got.Decision, got.PolicySource, got.Reason, tt.want, tt.wantSource)
			}
		})
	}
}

// A relaxed allow carries the constrained budget, never the generous low-risk
// one, and names the policy as its reason.
func TestDecideWithPoliciesRelaxedAllowIsConstrained(t *testing.T) {
	got := DecideWithPolicies(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite),
		[]ToolPolicy{policyOf(ToolPolicyScopeUser, ToolPolicyAllow, digestA)})
	if got.Decision != DecisionAllow || got.Reason != "tool_policy_allow" || got.BudgetContext != relaxedBudget {
		t.Fatalf("unexpected relaxed result %+v", got)
	}
}

// The inputs are never mutated: a caller holding the base result sees it as it
// was.
func TestDecideWithPoliciesDoesNotMutateBase(t *testing.T) {
	base := askBase()
	_ = DecideWithPolicies(base, mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite),
		[]ToolPolicy{policyOf(ToolPolicyScopeUser, ToolPolicyAllow, digestA)})
	if base.Decision != DecisionAsk || base.PolicySource != PolicySourceRiskLevel {
		t.Fatalf("base was mutated: %+v", base)
	}
}

func TestCanRelax(t *testing.T) {
	tests := []struct {
		scope, class, floor string
		want                bool
	}{
		{ToolPolicyScopeUser, models.SideEffectRead, models.ApprovalFloorNone, true},
		{ToolPolicyScopeUser, models.SideEffectExternalWrite, models.ApprovalFloorNone, true},
		{ToolPolicyScopeUser, models.SideEffectDestructive, models.ApprovalFloorNone, false},
		{ToolPolicyScopeOrg, models.SideEffectDestructive, models.ApprovalFloorNone, true},
		{ToolPolicyScopeOrg, models.SideEffectOwnerEffect, models.ApprovalFloorDurableOwner, false},
		{ToolPolicyScopeOrg, models.SideEffectOwnerEffect, models.ApprovalFloorNone, false},
		{ToolPolicyScopeOrg, models.SideEffectUnclassified, models.ApprovalFloorNone, false},
		{ToolPolicyScopeOrg, "not-a-class", models.ApprovalFloorNone, false},
		{ToolPolicyScopeOrg, models.SideEffectRead, models.ApprovalFloorAsk, false},
		{ToolPolicyScopeOrg, models.SideEffectRead, "", false},
		{"", models.SideEffectRead, models.ApprovalFloorNone, false},
		{"workspace", models.SideEffectRead, models.ApprovalFloorNone, false},
	}
	for _, tt := range tests {
		if got := CanRelax(tt.scope, tt.class, tt.floor); got != tt.want {
			t.Errorf("CanRelax(%q, %q, %q) = %v, want %v", tt.scope, tt.class, tt.floor, got, tt.want)
		}
	}
}

func TestToolPolicyEnforcedFromLookupIsStrict(t *testing.T) {
	for value, want := range map[string]bool{
		"true": true, "": false, "false": false, "1": false, "yes": false, "TRUE": false, "True": false, " true": false, "true ": false, "on": false,
	} {
		got := ToolPolicyEnforcedFromLookup(func(key string) string {
			if key != "TOOL_POLICY_ENFORCED" {
				t.Fatalf("read unexpected variable %q", key)
			}
			return value
		})
		if got != want {
			t.Errorf("%q: got %v, want %v", value, got, want)
		}
	}
}

type fakePolicySource struct {
	policies []ToolPolicy
	err      error
	calls    int
	gotOrg   string
	gotUser  string
	gotCap   string
}

func (f *fakePolicySource) ActiveToolPolicies(_ context.Context, orgID, userID, capabilityID string) ([]ToolPolicy, error) {
	f.calls++
	f.gotOrg, f.gotUser, f.gotCap = orgID, userID, capabilityID
	return f.policies, f.err
}

func evaluateTool(t *testing.T, engine *Engine, capability *models.Capability, binding *ToolBinding) (*Result, error) {
	t.Helper()
	return engine.EvaluateTool(context.Background(), capability, binding, "user-1", "run-1", "agent-1", "org-1", "global")
}

func TestEvaluateToolPoliciesAreIgnoredUnlessEnforced(t *testing.T) {
	capability := mcpCapability(models.RiskHigh)
	binding := mcpBinding(models.SideEffectExternalWrite)
	never := []ToolPolicy{policyOf(ToolPolicyScopeOrg, ToolPolicyNever, "")}

	t.Run("off: the source is never read and the decision is Decide's", func(t *testing.T) {
		source := &fakePolicySource{policies: never}
		got, err := evaluateTool(t, New(nil).WithToolPolicies(source, false), capability, binding)
		if err != nil || got.Decision != DecisionAsk || source.calls != 0 {
			t.Fatalf("got %+v err=%v calls=%d", got, err, source.calls)
		}
	})
	t.Run("a nil source can never be enforced", func(t *testing.T) {
		got, err := evaluateTool(t, New(nil).WithToolPolicies(nil, true), capability, binding)
		if err != nil || got.Decision != DecisionAsk {
			t.Fatalf("got %+v err=%v", got, err)
		}
	})
	t.Run("on: the org's and the person's policies apply", func(t *testing.T) {
		source := &fakePolicySource{policies: never}
		got, err := evaluateTool(t, New(nil).WithToolPolicies(source, true), capability, binding)
		if err != nil || got.Decision != DecisionDeny || got.PolicySource != PolicySourceToolPolicyNever {
			t.Fatalf("got %+v err=%v", got, err)
		}
		if source.gotOrg != "org-1" || source.gotUser != "user-1" || source.gotCap != capability.ID {
			t.Fatalf("source asked for org=%q user=%q capability=%q", source.gotOrg, source.gotUser, source.gotCap)
		}
	})
	t.Run("on: an unreadable store fails the evaluation instead of deciding without it", func(t *testing.T) {
		source := &fakePolicySource{err: errors.New("db down")}
		got, err := evaluateTool(t, New(nil).WithToolPolicies(source, true), capability, binding)
		if err == nil || got != nil {
			t.Fatalf("got %+v err=%v, want an error", got, err)
		}
	})
	t.Run("on: a user allow relaxes the pinned MCP write", func(t *testing.T) {
		source := &fakePolicySource{policies: []ToolPolicy{policyOf(ToolPolicyScopeUser, ToolPolicyAllow, digestA)}}
		got, err := evaluateTool(t, New(nil).WithToolPolicies(source, true), capability, binding)
		if err != nil || got.Decision != DecisionAllow || got.PolicySource != PolicySourceToolPolicyAllow {
			t.Fatalf("got %+v err=%v", got, err)
		}
	})
}

// The request and gate checks come before any policy: a denied capability never
// reaches the store, and a bad request is an error whatever the policies say.
func TestEvaluateToolGatesBeforePolicies(t *testing.T) {
	source := &fakePolicySource{policies: []ToolPolicy{policyOf(ToolPolicyScopeUser, ToolPolicyAllow, digestA)}}
	engine := New(nil).WithToolPolicies(source, true)
	scoped := mcpCapability(models.RiskHigh)
	scoped.EnabledForScopes = []string{"user"}
	got, err := evaluateTool(t, engine, scoped, mcpBinding(models.SideEffectExternalWrite))
	if err != nil || got.Decision != DecisionDeny || got.PolicySource != PolicySourceCapabilityGate || source.calls != 0 {
		t.Fatalf("got %+v err=%v calls=%d", got, err, source.calls)
	}
	if _, err := engine.EvaluateTool(context.Background(), mcpCapability(models.RiskHigh), nil, "user-1", "", "agent-1", "org-1", "global"); err == nil {
		t.Fatal("a request without a run id was evaluated")
	}
}

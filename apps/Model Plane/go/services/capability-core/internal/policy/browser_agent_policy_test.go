package policy

import (
	"errors"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

func TestBrowserAgentScopedPolicyIsStrictlyOptIn(t *testing.T) {
	for _, value := range []string{"", "false", "1", "TRUE", " true"} {
		if BrowserAgentPolicyEnforcedFromLookup(func(string) string { return value }) {
			t.Fatalf("%q enabled", value)
		}
	}
	if !BrowserAgentPolicyEnforcedFromLookup(func(string) string { return "true" }) {
		t.Fatal("literal true did not enable")
	}
}

func TestBrowserAgentScopedPoliciesApplyOnlyToTheNamedTool(t *testing.T) {
	cap := mcpCapability(models.RiskHigh)
	cap.ID = "cap.browser.open"
	cap.Kind = models.KindTool
	browser := &ToolBinding{ToolName: "browser_agent", CapabilityID: cap.ID, SideEffectClass: models.SideEffectExternalWrite, ApprovalFloor: models.ApprovalFloorNone}
	for _, scope := range []string{ToolPolicyScopeOrg, ToolPolicyScopeUser} {
		for _, test := range []struct{ decision, want, source string }{
			{ToolPolicyNever, DecisionDeny, PolicySourceToolPolicyNever},
			{ToolPolicyAsk, DecisionAsk, PolicySourceToolPolicyAsk},
			{ToolPolicyAllow, DecisionAsk, PolicySourceRiskLevel},
		} {
			t.Run(scope+"/"+test.decision, func(t *testing.T) {
				store := &fakePolicySource{policies: []ToolPolicy{policyOf(scope, test.decision, "")}}
				engine := New(nil).WithToolPolicies(store, false).WithBrowserAgentPolicyEnforced(true)
				got, err := evaluateTool(t, engine, cap, browser)
				if err != nil || got.Decision != test.want || got.PolicySource != test.source || !got.ToolPolicyChecked {
					t.Fatalf("got %+v err=%v", got, err)
				}
				if store.calls != 1 || store.gotOrg != "org-1" || store.gotUser != "user-1" || store.gotCap != cap.ID {
					t.Fatalf("wrong scoped lookup: %+v", store)
				}
			})
		}
	}
	for _, name := range []string{"browser.console", "web_fetch", "browser_agent_extra"} {
		store := &fakePolicySource{policies: []ToolPolicy{policyOf(ToolPolicyScopeOrg, ToolPolicyNever, "")}}
		engine := New(nil).WithToolPolicies(store, false).WithBrowserAgentPolicyEnforced(true)
		binding := &ToolBinding{ToolName: name, CapabilityID: cap.ID, SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone}
		got, err := evaluateTool(t, engine, cap, binding)
		if err != nil || got.Decision != DecisionAsk || got.ToolPolicyChecked || store.calls != 0 {
			t.Fatalf("%s changed: %+v err=%v calls=%d", name, got, err, store.calls)
		}
	}
	other := mcpCapability(models.RiskHigh)
	other.ID = "cap.other"
	store := &fakePolicySource{}
	got, err := evaluateTool(t, New(nil).WithToolPolicies(store, false).WithBrowserAgentPolicyEnforced(true), other, browser)
	if err != nil || got.ToolPolicyChecked || store.calls != 0 {
		t.Fatalf("other capability was scoped: %+v %v", got, err)
	}
}

func TestBrowserAgentScopedPolicyFailsClosed(t *testing.T) {
	cap := mcpCapability(models.RiskHigh)
	cap.ID = "cap.browser.open"
	cap.Kind = models.KindTool
	binding := &ToolBinding{ToolName: "browser_agent", CapabilityID: cap.ID, SideEffectClass: models.SideEffectExternalWrite, ApprovalFloor: models.ApprovalFloorNone}
	for _, source := range []ToolPolicySource{nil, &fakePolicySource{err: errors.New("database unavailable")}} {
		engine := New(nil).WithToolPolicies(source, false).WithBrowserAgentPolicyEnforced(true)
		if got, err := evaluateTool(t, engine, cap, binding); err == nil || got != nil {
			t.Fatalf("missing store accepted: %+v %v", got, err)
		}
	}
	engine := New(nil).WithToolPolicies(&fakePolicySource{}, false).WithBrowserAgentPolicyEnforced(true)
	if got, err := engine.EvaluateTool(t.Context(), cap, binding, "", "run-1", "agent-1", "org-1", "global"); err == nil || got != nil {
		t.Fatalf("missing owner accepted: %+v %v", got, err)
	}
}

package policy

import (
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// W2-WS13-S7-rule-use-b: a person's stored allow never relaxes a retention-bound run, an org allow
// still does, and the relaxing rule is named for the rule-use audit row.

func TestRetentionBoundFor(t *testing.T) {
	cases := []struct {
		retention string
		audit     bool
		want      bool
	}{
		{"retention_bound", false, true},
		{"retention_bound", true, true},
		{"", false, false},
		{"", true, true},
		{"persistent", false, false},
		{"persistent", true, false},
		// An unknown word is not "persistent": bound while the audit is on, as before while it is off.
		{"Persistent", true, true},
		{"Persistent", false, false},
		{"zdr", true, true},
	}
	for _, tc := range cases {
		if got := RetentionBoundFor(tc.retention, tc.audit); got != tc.want {
			t.Errorf("RetentionBoundFor(%q, %t) = %t, want %t", tc.retention, tc.audit, got, tc.want)
		}
	}
}

func TestToolPolicyRuleAuditIsOnlyTheLiteralTrue(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "": false, "TRUE": false, "1": false, "yes": false, "true ": false} {
		got := ToolPolicyRuleAuditFromLookup(func(name string) string {
			if name != "TOOL_POLICY_RULE_AUDIT_ENABLED" {
				t.Fatalf("read %q", name)
			}
			return value
		})
		if got != want {
			t.Errorf("%q -> %t, want %t", value, got, want)
		}
	}
}

func idPolicy(id, scope, decision, digest string) ToolPolicy {
	return ToolPolicy{ID: id, ScopeKind: scope, Decision: decision, PinnedDigest: digest}
}

func TestAUserAllowIsIgnoredOnARetentionBoundRun(t *testing.T) {
	policies := []ToolPolicy{idPolicy("tp_user", ToolPolicyScopeUser, ToolPolicyAllow, digestA)}
	got := DecideWithPoliciesFor(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite), policies,
		DecideOptions{RetentionBound: true})
	if got.Decision != DecisionAsk || got.PolicySource != PolicySourceRiskLevel || got.RulePolicyID != "" {
		t.Fatalf("a person's allow relaxed a retention-bound run: %+v", got)
	}
	// The same call on a persistent run is relaxed and names the rule.
	got = DecideWithPoliciesFor(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite), policies, DecideOptions{})
	if got.Decision != DecisionAllow || got.RulePolicyID != "tp_user" || got.RuleScope != ToolPolicyScopeUser {
		t.Fatalf("persistent run: %+v", got)
	}
}

func TestAnOrgAllowStillRelaxesARetentionBoundRun(t *testing.T) {
	policies := []ToolPolicy{
		idPolicy("tp_user", ToolPolicyScopeUser, ToolPolicyAllow, digestA),
		idPolicy("tp_org", ToolPolicyScopeOrg, ToolPolicyAllow, digestA),
	}
	got := DecideWithPoliciesFor(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite), policies,
		DecideOptions{RetentionBound: true})
	if got.Decision != DecisionAllow || got.PolicySource != PolicySourceToolPolicyAllow ||
		got.RulePolicyID != "tp_org" || got.RuleScope != ToolPolicyScopeOrg {
		t.Fatalf("org allow on a retention-bound run: %+v", got)
	}
}

func TestTheRelaxingRuleNamesTheUserRuleWhenBothApply(t *testing.T) {
	// Org first in the slice: the person's rule is still the one named.
	policies := []ToolPolicy{
		idPolicy("tp_org", ToolPolicyScopeOrg, ToolPolicyAllow, digestA),
		idPolicy("tp_user_1", ToolPolicyScopeUser, ToolPolicyAllow, digestA),
		idPolicy("tp_user_2", ToolPolicyScopeUser, ToolPolicyAllow, digestA),
	}
	got := DecideWithPoliciesFor(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite), policies, DecideOptions{})
	if got.Decision != DecisionAllow || got.RulePolicyID != "tp_user_1" || got.RuleScope != ToolPolicyScopeUser {
		t.Fatalf("got %+v, want tp_user_1/user", got)
	}
}

func TestAStaleUserAllowIsNotNamedAsTheRelaxingRule(t *testing.T) {
	policies := []ToolPolicy{
		idPolicy("tp_user_stale", ToolPolicyScopeUser, ToolPolicyAllow, digestB),
		idPolicy("tp_org", ToolPolicyScopeOrg, ToolPolicyAllow, digestA),
	}
	got := DecideWithPoliciesFor(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite), policies, DecideOptions{})
	// A stale allow reads as an ask, so nothing is relaxed and no rule is named.
	if got.Decision != DecisionAsk || got.RulePolicyID != "" {
		t.Fatalf("got %+v", got)
	}
}

func TestNoRuleIsNamedWhenNothingWasRelaxed(t *testing.T) {
	// A base allow is not relaxed by an allow; no rule-use row may be written for it.
	base := &Result{Decision: DecisionAllow, Reason: "low", PolicySource: PolicySourceRiskLevel}
	policies := []ToolPolicy{idPolicy("tp_user", ToolPolicyScopeUser, ToolPolicyAllow, digestA)}
	got := DecideWithPoliciesFor(base, mcpCapability(models.RiskLow), mcpBinding(models.SideEffectExternalWrite), policies, DecideOptions{})
	if got.RulePolicyID != "" || got.PolicySource == PolicySourceToolPolicyAllow {
		t.Fatalf("a base allow was reported as rule-relaxed: %+v", got)
	}
}

// DecideWithPolicies is DecideWithPoliciesFor with no options: an existing caller sees a person's
// allow applied as before.
func TestDecideWithPoliciesIsTheUnboundWrapper(t *testing.T) {
	policies := []ToolPolicy{idPolicy("tp_user", ToolPolicyScopeUser, ToolPolicyAllow, digestA)}
	a := DecideWithPolicies(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite), policies)
	b := DecideWithPoliciesFor(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite), policies, DecideOptions{})
	if *a != *b {
		t.Fatalf("wrapper %+v differs from %+v", a, b)
	}
}

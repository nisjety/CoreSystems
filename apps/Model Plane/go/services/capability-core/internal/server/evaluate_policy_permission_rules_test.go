package server

import (
	"context"
	"errors"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/api"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
)

// W5-074-b: the org permission rules tighten the EvaluatePolicy decision last, only while
// CAPABILITY_PERMISSION_RULES_ENABLED is on, and never loosen it.

// ruleMatcher matches through the real api.MatchPermissionRules over a fixed rule set, and records
// what the gate asked for.
type ruleMatcher struct {
	rules []api.PermissionRule
	err   error
	calls int
	got   [4]string
}

func (m *ruleMatcher) Match(_ context.Context, orgID, tool, kind, value string) (string, string, error) {
	m.calls++
	m.got = [4]string{orgID, tool, kind, value}
	if m.err != nil {
		return "", "", m.err
	}
	decision, ruleID := api.MatchPermissionRules(m.rules, tool, kind, value)
	return decision, ruleID, nil
}

const (
	denyRuleID = "11111111-1111-4111-8111-111111111111"
	askRuleID  = "22222222-2222-4222-8222-222222222222"
)

func permissionRulesServer(t *testing.T, class, floor string, matcher *ruleMatcher, enabled bool) (*Server, *DecisionProofSigner, string) {
	t.Helper()
	capability := availableCapabilityRow("cap.tool.http", models.KindTool, models.RiskLow, class, floor)
	db := &bindingDB{rows: []bindingRow{{capabilityID: capability.ID, toolName: "web_fetch", class: class, floor: floor, version: "1.0.0"}}}
	server, signer := toolBindingServer(t, db, capability)
	server.WithPermissionRules(matcher, enabled)
	return server, signer, capability.ID
}

func evaluateTarget(t *testing.T, server *Server, capabilityID, kind, value, retention string) *mpv1.EvaluatePolicyResponse {
	t.Helper()
	response, err := server.EvaluatePolicy(verifiedGRPCContext(t, "org-a"), &mpv1.EvaluatePolicyRequest{
		CapabilityId: capabilityID, RunId: "run-a", AgentId: "execution-core", Scope: "global",
		ToolName: "web_fetch", UserId: "user-a", TargetKind: kind, TargetValue: value, RunRetention: retention,
	})
	if err != nil {
		t.Fatalf("EvaluatePolicy: %v", err)
	}
	return response
}

func TestPermissionRulesAreNotReadWhileTheSwitchIsOff(t *testing.T) {
	matcher := &ruleMatcher{rules: []api.PermissionRule{{ID: denyRuleID, ToolPattern: "*", TargetKind: "any", TargetPattern: "*", Decision: "deny"}}}
	server, _, id := permissionRulesServer(t, models.SideEffectRead, models.ApprovalFloorNone, matcher, false)
	response := evaluateTarget(t, server, id, "domain", "blocked.example", "persistent")
	if response.Decision != policy.DecisionAllow || response.PolicySource != policy.PolicySourceRiskLevel {
		t.Fatalf("decision changed while the switch was off: %+v", response)
	}
	if matcher.calls != 0 {
		t.Fatalf("rules were read while the switch was off (%d calls)", matcher.calls)
	}
}

func TestPermissionRulesTightenAnAllow(t *testing.T) {
	for _, tc := range []struct {
		name, wantDecision, wantRule string
		rules                        []api.PermissionRule
	}{
		{"a deny rule denies", policy.DecisionDeny, denyRuleID, []api.PermissionRule{
			{ID: denyRuleID, ToolPattern: "web_*", TargetKind: "domain", TargetPattern: "*.example", Decision: "deny"}}},
		{"an ask rule asks", policy.DecisionAsk, askRuleID, []api.PermissionRule{
			{ID: askRuleID, ToolPattern: "web_fetch", TargetKind: "domain", TargetPattern: "blocked.example", Decision: "ask"}}},
		{"deny outranks an earlier ask", policy.DecisionDeny, denyRuleID, []api.PermissionRule{
			{ID: askRuleID, ToolPattern: "*", TargetKind: "any", TargetPattern: "*", Decision: "ask"},
			{ID: denyRuleID, ToolPattern: "web_fetch", TargetKind: "domain", TargetPattern: "*", Decision: "deny"}}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			matcher := &ruleMatcher{rules: tc.rules}
			server, signer, id := permissionRulesServer(t, models.SideEffectRead, models.ApprovalFloorNone, matcher, true)
			response := evaluateTarget(t, server, id, "domain", "Blocked.Example", "persistent")
			if response.Decision != tc.wantDecision || response.Reason != "org_rule:"+tc.wantRule ||
				response.PolicySource != policy.PolicySourceOrgPermissionRule {
				t.Fatalf("decision=%q reason=%q source=%q", response.Decision, response.Reason, response.PolicySource)
			}
			if matcher.got != [4]string{"org-a", "web_fetch", "domain", "Blocked.Example"} {
				t.Fatalf("the gate asked for %v", matcher.got)
			}
			claims := evidenceClaims(t, signer, response.DecisionEvidence)
			if claims["decision"] != tc.wantDecision || claims["policy_source"] != policy.PolicySourceOrgPermissionRule {
				t.Fatalf("evidence disagrees with the response: %v", claims)
			}
			// The signed decision names the rule and nothing of the call's target.
			for _, value := range []any{response.Reason, claims["reason"]} {
				if s, _ := value.(string); s != "" && (containsAny(s, "example", "Blocked")) {
					t.Fatalf("the decision carries the call's target: %q", s)
				}
			}
		})
	}
}

func TestPermissionRulesNeverLoosen(t *testing.T) {
	// A deny stands and is not even matched.
	matcher := &ruleMatcher{}
	server, _, id := permissionRulesServer(t, models.SideEffectRead, models.ApprovalFloorNone, matcher, true)
	server.policy.WithToolPolicies(&toolPolicyStore{policies: []policy.ToolPolicy{{ScopeKind: policy.ToolPolicyScopeOrg, Decision: policy.ToolPolicyNever}}}, true)
	response := evaluateTarget(t, server, id, "domain", "fine.example", "persistent")
	if response.Decision != policy.DecisionDeny || response.PolicySource != policy.PolicySourceToolPolicyNever {
		t.Fatalf("a deny was rewritten: %+v", response)
	}
	if matcher.calls != 0 {
		t.Fatalf("rules were read for a call already denied (%d calls)", matcher.calls)
	}

	// No matching rule leaves an allow as it was.
	other := &ruleMatcher{rules: []api.PermissionRule{{ID: denyRuleID, ToolPattern: "web_fetch", TargetKind: "domain", TargetPattern: "other.example", Decision: "deny"}}}
	server, _, id = permissionRulesServer(t, models.SideEffectRead, models.ApprovalFloorNone, other, true)
	response = evaluateTarget(t, server, id, "domain", "fine.example", "persistent")
	if response.Decision != policy.DecisionAllow || response.PolicySource != policy.PolicySourceRiskLevel {
		t.Fatalf("an unmatched rule changed the decision: %+v", response)
	}
}

// A failed read of the table asks for an effect class and leaves a read class unchanged.
func TestPermissionRulesUnreadableFailsClosedForEffectClassesOnly(t *testing.T) {
	matcher := &ruleMatcher{err: errors.New("db down")}
	server, _, id := permissionRulesServer(t, models.SideEffectContextWrite, models.ApprovalFloorNone, matcher, true)
	response := evaluateTarget(t, server, id, "", "", "persistent")
	if response.Decision != policy.DecisionAsk || response.Reason != "org_rules_unavailable" ||
		response.PolicySource != policy.PolicySourceOrgPermissionRule {
		t.Fatalf("effect class with unreadable rules: %+v", response)
	}

	readMatcher := &ruleMatcher{err: errors.New("db down")}
	server, _, id = permissionRulesServer(t, models.SideEffectRead, models.ApprovalFloorNone, readMatcher, true)
	response = evaluateTarget(t, server, id, "", "", "persistent")
	if response.Decision != policy.DecisionAllow || response.PolicySource != policy.PolicySourceRiskLevel {
		t.Fatalf("read class with unreadable rules changed: %+v", response)
	}
}

// A ZDR or waived run sends the target kind and withholds the value (execution-core
// capability_policy.rs a_retention_bound_call_names_no_target_value). ZDR is a floor: a run that
// stores nothing may not be decided more loosely than a persistent call to the same target. An
// org rule that denies one domain must therefore still stop (or at least pause) a retention-bound
// web_fetch whose domain capability-core cannot see.
func TestPermissionRulesStillTightenARetentionBoundCallWithAWithheldTarget(t *testing.T) {
	rules := []api.PermissionRule{{ID: denyRuleID, ToolPattern: "web_fetch", TargetKind: "domain", TargetPattern: "blocked.example", Decision: "deny"}}

	persistent := &ruleMatcher{rules: rules}
	server, _, id := permissionRulesServer(t, models.SideEffectRead, models.ApprovalFloorNone, persistent, true)
	if response := evaluateTarget(t, server, id, "domain", "blocked.example", "persistent"); response.Decision != policy.DecisionDeny {
		t.Fatalf("persistent call to the denied domain: %+v", response)
	}

	bound := &ruleMatcher{rules: rules}
	server, _, id = permissionRulesServer(t, models.SideEffectRead, models.ApprovalFloorNone, bound, true)
	response := evaluateTarget(t, server, id, "domain", "", "retention_bound")
	if response.Decision == policy.DecisionAllow {
		t.Fatalf("a retention-bound call with a withheld domain was allowed past the org's domain deny rule: %+v", response)
	}
}

// A withheld target (a ZDR or waived run sends the kind and no value) is decided by the rules of its kind as a "*"
// pattern decides it: a deny of the kind denies and an ask of the kind asks. A rule of another kind does not cover it,
// a call that names no kind has no kind rule, and with the switch off no rule is read.
func TestPermissionRulesDecideAWithheldTargetByTheRulesOfItsKind(t *testing.T) {
	denyDomain := []api.PermissionRule{{ID: denyRuleID, ToolPattern: "web_fetch", TargetKind: "domain", TargetPattern: "blocked.example", Decision: "deny"}}
	askDomain := []api.PermissionRule{{ID: askRuleID, ToolPattern: "web_fetch", TargetKind: "domain", TargetPattern: "*.example", Decision: "ask"}}
	recipientDeny := []api.PermissionRule{{ID: denyRuleID, ToolPattern: "*", TargetKind: "recipient", TargetPattern: "*", Decision: "deny"}}
	for _, tc := range []struct {
		name, kind, value, retention string
		rules                        []api.PermissionRule
		wantDecision, wantRule       string
	}{
		{"a deny of the kind denies the withheld target", "domain", "", "retention_bound", denyDomain, policy.DecisionDeny, denyRuleID},
		{"an ask of the kind asks for the withheld target", "domain", "", "retention_bound", askDomain, policy.DecisionAsk, askRuleID},
		{"a rule of another kind does not cover it", "domain", "", "retention_bound", recipientDeny, policy.DecisionAllow, ""},
		{"a call that names no kind has no kind rule", "", "", "retention_bound", denyDomain, policy.DecisionAllow, ""},
		{"a persistent call to another domain is unchanged", "domain", "fine.example", "persistent", denyDomain, policy.DecisionAllow, ""},
	} {
		t.Run(tc.name, func(t *testing.T) {
			matcher := &ruleMatcher{rules: tc.rules}
			server, signer, id := permissionRulesServer(t, models.SideEffectRead, models.ApprovalFloorNone, matcher, true)
			response := evaluateTarget(t, server, id, tc.kind, tc.value, tc.retention)
			if matcher.got != [4]string{"org-a", "web_fetch", tc.kind, tc.value} {
				t.Fatalf("the gate asked for %v", matcher.got)
			}
			if tc.wantRule == "" {
				if response.Decision != policy.DecisionAllow || response.PolicySource != policy.PolicySourceRiskLevel {
					t.Fatalf("a rule that does not cover the call changed it: %+v", response)
				}
				return
			}
			if response.Decision != tc.wantDecision || response.Reason != "org_rule:"+tc.wantRule ||
				response.PolicySource != policy.PolicySourceOrgPermissionRule {
				t.Fatalf("decision=%q reason=%q source=%q", response.Decision, response.Reason, response.PolicySource)
			}
			if claims := evidenceClaims(t, signer, response.DecisionEvidence); claims["decision"] != tc.wantDecision ||
				claims["policy_source"] != policy.PolicySourceOrgPermissionRule {
				t.Fatalf("evidence disagrees with the response: %v", claims)
			}
		})
	}

	// The switch off reads no rule, so the withheld call is decided as it was before the rules existed.
	matcher := &ruleMatcher{rules: denyDomain}
	server, _, id := permissionRulesServer(t, models.SideEffectRead, models.ApprovalFloorNone, matcher, false)
	if response := evaluateTarget(t, server, id, "domain", "", "retention_bound"); response.Decision != policy.DecisionAllow ||
		response.PolicySource != policy.PolicySourceRiskLevel || matcher.calls != 0 {
		t.Fatalf("a withheld call with the switch off changed or read the rules: %+v (%d reads)", response, matcher.calls)
	}
}

func containsAny(s string, needles ...string) bool {
	for _, needle := range needles {
		for i := 0; i+len(needle) <= len(s); i++ {
			if s[i:i+len(needle)] == needle {
				return true
			}
		}
	}
	return false
}

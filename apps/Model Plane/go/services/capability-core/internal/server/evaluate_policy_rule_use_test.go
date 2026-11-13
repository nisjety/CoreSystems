package server

import (
	"context"
	"errors"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/api"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
)

// W2-WS13-S7-rule-use-b: EvaluatePolicy writes one content-free tool_policy_rule_used row for a
// call a stored allow relaxed on a persistent run, writes nothing otherwise, and fails the call
// closed (an ask) when the row cannot be written.

type ruleUseRecorder struct {
	uses []api.RuleUse
	err  error
}

func (r *ruleUseRecorder) RecordRuleUse(_ context.Context, use api.RuleUse) error {
	r.uses = append(r.uses, use)
	return r.err
}

// ruleUseServer is a high-risk MCP tool (a risk ask) whose approved, pinned binding a stored
// allow may relax.
func ruleUseServer(t *testing.T, policies []policy.ToolPolicy, recorder RuleUseRecorder, audit bool) (*Server, string) {
	t.Helper()
	capability := availableCapabilityRow("cap.mcp_tool.x", models.KindMCPTool, models.RiskHigh, models.SideEffectExternalWrite, models.ApprovalFloorNone)
	db := &bindingDB{rows: []bindingRow{{
		capabilityID: capability.ID, toolName: "mcp__s__t", class: models.SideEffectExternalWrite, floor: models.ApprovalFloorNone,
		digest: testDigest, reviewState: policy.ReviewStateApproved, version: "1.0.0",
	}}}
	server, _ := toolBindingServer(t, db, capability)
	server.policy.WithToolPolicies(&toolPolicyStore{policies: policies}, true)
	server.WithRuleAudit(recorder, audit)
	return server, capability.ID
}

func evaluateRuleUse(t *testing.T, ctx context.Context, server *Server, capabilityID, runID, retention string) *mpv1.EvaluatePolicyResponse {
	t.Helper()
	response, err := server.EvaluatePolicy(ctx, &mpv1.EvaluatePolicyRequest{
		CapabilityId: capabilityID, RunId: runID, AgentId: "execution-core", Scope: "global",
		ToolName: "mcp__s__t", UserId: "user-a", RunRetention: retention,
	})
	if err != nil {
		t.Fatalf("EvaluatePolicy: %v", err)
	}
	return response
}

func userAllow(id string) policy.ToolPolicy {
	return policy.ToolPolicy{ID: id, ScopeKind: policy.ToolPolicyScopeUser, Decision: policy.ToolPolicyAllow, PinnedDigest: testDigest}
}

func orgAllow(id string) policy.ToolPolicy {
	return policy.ToolPolicy{ID: id, ScopeKind: policy.ToolPolicyScopeOrg, Decision: policy.ToolPolicyAllow, PinnedDigest: testDigest}
}

func TestRuleUseIsRecordedForAPersistentRun(t *testing.T) {
	recorder := &ruleUseRecorder{}
	server, id := ruleUseServer(t, []policy.ToolPolicy{userAllow("tp_user")}, recorder, true)
	response := evaluateRuleUse(t, verifiedGRPCContext(t, "org-a"), server, id, "run-a", "persistent")
	if response.Decision != policy.DecisionAllow || response.PolicySource != policy.PolicySourceToolPolicyAllow {
		t.Fatalf("decision = %+v", response)
	}
	if len(recorder.uses) != 1 {
		t.Fatalf("uses = %+v, want one", recorder.uses)
	}
	use := recorder.uses[0]
	want := api.RuleUse{OrgID: "org-a", PolicyID: "tp_user", CapabilityID: id, ScopeKind: policy.ToolPolicyScopeUser,
		RunID: "run-a", ToolDigest: testDigest, Actor: "execution-core"}
	if use.OrgID != want.OrgID || use.PolicyID != want.PolicyID || use.CapabilityID != want.CapabilityID ||
		use.ScopeKind != want.ScopeKind || use.RunID != want.RunID || use.ToolDigest != want.ToolDigest || use.Actor == "" {
		t.Fatalf("use = %+v, want %+v (actor the service principal)", use, want)
	}
}

func TestRuleUseWritesNothingWhileTheAuditIsOff(t *testing.T) {
	recorder := &ruleUseRecorder{}
	server, id := ruleUseServer(t, []policy.ToolPolicy{userAllow("tp_user")}, recorder, false)
	// Off, an unstated posture is not bound: the person's allow applies as before, with no row.
	response := evaluateRuleUse(t, verifiedGRPCContext(t, "org-a"), server, id, "run-a", "")
	if response.Decision != policy.DecisionAllow || len(recorder.uses) != 0 {
		t.Fatalf("audit off: decision=%q uses=%+v", response.Decision, recorder.uses)
	}
}

func TestRuleUseOnARetentionBoundRun(t *testing.T) {
	for _, retention := range []string{"retention_bound", ""} {
		t.Run("person's allow, retention "+retention, func(t *testing.T) {
			recorder := &ruleUseRecorder{}
			server, id := ruleUseServer(t, []policy.ToolPolicy{userAllow("tp_user")}, recorder, true)
			response := evaluateRuleUse(t, verifiedGRPCContext(t, "org-a"), server, id, "run-a", retention)
			if response.Decision != policy.DecisionAsk || len(recorder.uses) != 0 {
				t.Fatalf("decision=%q source=%q uses=%+v; want the base ask and no row", response.Decision, response.PolicySource, recorder.uses)
			}
		})
		t.Run("org allow, retention "+retention, func(t *testing.T) {
			recorder := &ruleUseRecorder{}
			server, id := ruleUseServer(t, []policy.ToolPolicy{orgAllow("tp_org")}, recorder, true)
			response := evaluateRuleUse(t, verifiedGRPCContext(t, "org-a"), server, id, "run-a", retention)
			if response.Decision != policy.DecisionAllow || len(recorder.uses) != 0 {
				t.Fatalf("decision=%q uses=%+v; want the org allow applied and no row (DEC-32)", response.Decision, recorder.uses)
			}
		})
	}
	// Even with the audit off, an explicit retention_bound keeps a person's allow off the call.
	recorder := &ruleUseRecorder{}
	server, id := ruleUseServer(t, []policy.ToolPolicy{userAllow("tp_user")}, recorder, false)
	if response := evaluateRuleUse(t, verifiedGRPCContext(t, "org-a"), server, id, "run-a", "retention_bound"); response.Decision != policy.DecisionAsk {
		t.Fatalf("audit off, retention_bound: %+v", response)
	}
}

func TestRuleUseFailedWriteFailsTheCallClosed(t *testing.T) {
	for name, recorder := range map[string]RuleUseRecorder{
		"write error":     &ruleUseRecorder{err: errors.New("db down")},
		"no recorder set": nil,
	} {
		t.Run(name, func(t *testing.T) {
			server, id := ruleUseServer(t, []policy.ToolPolicy{userAllow("tp_user")}, recorder, true)
			response := evaluateRuleUse(t, verifiedGRPCContext(t, "org-a"), server, id, "run-a", "persistent")
			if response.Decision != policy.DecisionAsk || response.Reason != "tool_policy_rule_unaudited" ||
				response.PolicySource != policy.PolicySourceRiskLevel {
				t.Fatalf("decision=%q reason=%q source=%q; want ask tool_policy_rule_unaudited risk_level",
					response.Decision, response.Reason, response.PolicySource)
			}
		})
	}
}

// A preview (a person evaluating their own call) and a call without a run write nothing.
func TestRuleUseWritesNothingForAPreviewOrAMissingRun(t *testing.T) {
	recorder := &ruleUseRecorder{}
	server, id := ruleUseServer(t, []policy.ToolPolicy{userAllow("tp_user")}, recorder, true)
	person := authctx.ContextWithPrincipal(context.Background(), authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"})
	_ = evaluateRuleUse(t, person, server, id, "run-a", "persistent")
	// A call without a run is refused before any decision (the gate needs a run); no row either way.
	_, _ = server.EvaluatePolicy(verifiedGRPCContext(t, "org-a"), &mpv1.EvaluatePolicyRequest{
		CapabilityId: id, AgentId: "execution-core", Scope: "global", ToolName: "mcp__s__t", UserId: "user-a", RunRetention: "persistent",
	})
	if len(recorder.uses) != 0 {
		t.Fatalf("uses = %+v, want none", recorder.uses)
	}
}

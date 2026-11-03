package policy_test

import (
	"context"
	"errors"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
)

// fakeOrgExtensions is an in-memory OrgExtensionSource that counts its reads.
type fakeOrgExtensions struct {
	rule  policy.OrgExtensionRule
	err   error
	reads int
}

func (f *fakeOrgExtensions) OrgExtensionRule(context.Context, string) (policy.OrgExtensionRule, error) {
	f.reads++
	return f.rule, f.err
}

const orgExtensionOrg = "org-extensions"

// mcpToolCapability is one MCP tool's capability, enabled for the org scope the way every tool the gate evaluates is.
func mcpToolCapability() *models.Capability {
	return &models.Capability{
		ID: "cap.mcp_tool.ext", Name: "MCP tool", Kind: models.KindMCPTool, Version: "1.0.0",
		RiskLevel: models.RiskLow, Scope: "org", Enabled: true,
		EnabledForScopes: []string{"org"}, SideEffectClass: models.SideEffectRead,
	}
}

// approvedBinding is the registry's row for an approved MCP tool on the named server, pinned to its definition.
func approvedBinding(serverID string) *policy.ToolBinding {
	return &policy.ToolBinding{
		ToolName: "lookup", CapabilityID: "cap.mcp_tool.ext", CapabilityVersion: "1.0.0",
		SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone,
		ToolDigest: "sha256:pinned", ReviewState: policy.ReviewStateApproved, ServerID: serverID,
	}
}

func evaluateMCPTool(t *testing.T, source policy.OrgExtensionSource, enabled bool, binding *policy.ToolBinding) (*policy.Result, error) {
	t.Helper()
	engine := policy.New(nil).WithOrgExtensionPolicy(source, enabled)
	return engine.EvaluateTool(context.Background(), mcpToolCapability(), binding, "user-1", "run-1", "agent-1", orgExtensionOrg, "org")
}

func requireOrgExtensionDenial(t *testing.T, result *policy.Result, err error) {
	t.Helper()
	if err != nil {
		t.Fatalf("EvaluateTool error = %v, want a decision", err)
	}
	if result.Decision != policy.DecisionDeny || result.Reason != policy.ReasonOrgExtensionNotAllowed {
		t.Fatalf("decision = %s %q, want deny %q", result.Decision, result.Reason, policy.ReasonOrgExtensionNotAllowed)
	}
	if result.PolicySource != policy.PolicySourceCapabilityGate {
		t.Fatalf("policy source = %q, want %q", result.PolicySource, policy.PolicySourceCapabilityGate)
	}
}

func TestOrgExtensionNoneDeniesEveryMCPToolAndLeavesBuiltInToolsAlone(t *testing.T) {
	source := &fakeOrgExtensions{rule: policy.OrgExtensionRule{ServersMode: policy.OrgServersNone}}

	result, err := evaluateMCPTool(t, source, true, approvedBinding("srv-a"))
	requireOrgExtensionDenial(t, result, err)

	builtIn := &models.Capability{
		ID: "cap.test.built-in", Name: "Built-in", Kind: models.KindTool, Version: "1.0.0",
		RiskLevel: models.RiskLow, Scope: "org", Enabled: true, EnabledForScopes: []string{"org"},
	}
	engine := policy.New(nil).WithOrgExtensionPolicy(source, true)
	allowed, err := engine.EvaluateTool(context.Background(), builtIn, nil, "user-1", "run-1", "agent-1", orgExtensionOrg, "org")
	if err != nil || allowed.Decision != policy.DecisionAllow {
		t.Fatalf("built-in tool = %v, %v; want allow, an extension rule does not reach it", allowed, err)
	}
}

func TestOrgExtensionApprovedOnlyAllowsTheNamedServerAndNoOther(t *testing.T) {
	source := &fakeOrgExtensions{rule: policy.OrgExtensionRule{
		ServersMode: policy.OrgServersApprovedOnly,
		ServerIDs:   []string{"srv-a"},
	}}

	allowed, err := evaluateMCPTool(t, source, true, approvedBinding("srv-a"))
	if err != nil || allowed.Decision != policy.DecisionAllow {
		t.Fatalf("named server = %v, %v; want allow", allowed, err)
	}
	other, err := evaluateMCPTool(t, source, true, approvedBinding("srv-b"))
	requireOrgExtensionDenial(t, other, err)
	unknown, err := evaluateMCPTool(t, source, true, approvedBinding(""))
	requireOrgExtensionDenial(t, unknown, err)
}

func TestOrgExtensionRuleIsNotReadWhileTheSwitchIsOff(t *testing.T) {
	source := &fakeOrgExtensions{rule: policy.OrgExtensionRule{ServersMode: policy.OrgServersNone}}

	result, err := evaluateMCPTool(t, source, false, approvedBinding("srv-a"))
	if err != nil || result.Decision != policy.DecisionAllow {
		t.Fatalf("switch off = %v, %v; want the decision it was before the rule existed", result, err)
	}
	if source.reads != 0 {
		t.Fatalf("source read %d times with the switch off, want none", source.reads)
	}
}

func TestOrgExtensionSourceFailureFailsTheEvaluation(t *testing.T) {
	source := &fakeOrgExtensions{err: errors.New("registry unavailable")}

	result, err := evaluateMCPTool(t, source, true, approvedBinding("srv-a"))
	if err == nil || result != nil {
		t.Fatalf("evaluation = %v, %v; want no decision when the org's rule cannot be read", result, err)
	}
}

func TestOrgExtensionApprovedOnlyToolsNeedAPinnedDefinition(t *testing.T) {
	source := &fakeOrgExtensions{rule: policy.OrgExtensionRule{
		ServersMode:              policy.OrgServersAny,
		ToolApprovalApprovedOnly: true,
	}}

	pinned, err := evaluateMCPTool(t, source, true, approvedBinding("srv-a"))
	if err != nil || pinned.Decision != policy.DecisionAllow {
		t.Fatalf("pinned tool = %v, %v; want allow", pinned, err)
	}

	unpinned := approvedBinding("srv-a")
	unpinned.ToolDigest = ""
	withdrawn, err := evaluateMCPTool(t, source, true, unpinned)
	if err != nil || withdrawn.Decision != policy.DecisionDeny || withdrawn.Reason != models.ReasonToolNotApproved {
		t.Fatalf("unpinned tool = %v, %v; want deny %q", withdrawn, err, models.ReasonToolNotApproved)
	}

	drifted := approvedBinding("srv-a")
	drifted.ReviewState = policy.ReviewStateDrifted
	changed, err := evaluateMCPTool(t, source, true, drifted)
	if err != nil || changed.Decision != policy.DecisionDeny || changed.Reason != models.ReasonToolDefinitionChanged {
		t.Fatalf("drifted tool = %v, %v; want deny %q", changed, err, models.ReasonToolDefinitionChanged)
	}
}

func TestOrgExtensionRuleIsReadFromTheOrgPolicyDocument(t *testing.T) {
	cases := []struct {
		name             string
		settings         map[string]any
		wantServers      string
		wantIDs          []string
		wantToolApproval bool
	}{
		{
			name:        "a document with no keys is the platform default",
			settings:    map[string]any{},
			wantServers: policy.OrgServersAny,
		},
		{
			name:        "the lockdown bundle allows no MCP server",
			settings:    map[string]any{"lockdown": true, "ext.mcp_servers": map[string]any{"mode": "any", "ids": []any{}}},
			wantServers: policy.OrgServersNone,
		},
		{
			name: "an approved_only list names only its readable ids",
			settings: map[string]any{"ext.mcp_servers": map[string]any{
				"mode": "approved_only",
				"ids":  []any{"srv-a", 7, ""},
			}},
			wantServers: policy.OrgServersApprovedOnly,
			wantIDs:     []string{"srv-a"},
		},
		{
			name:        "an allow-list it cannot read allows nothing",
			settings:    map[string]any{"ext.mcp_servers": "any"},
			wantServers: policy.OrgServersNone,
		},
		{
			name:             "tool approval is approved_only unless it names trust_annotations",
			settings:         map[string]any{"mcp_tool_approval": "approved_only"},
			wantServers:      policy.OrgServersAny,
			wantToolApproval: true,
		},
		{
			name:             "an unreadable tool approval mode requires approval",
			settings:         map[string]any{"mcp_tool_approval": 3},
			wantServers:      policy.OrgServersAny,
			wantToolApproval: true,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rule := policy.OrgExtensionRuleFromSettings(tc.settings)
			if rule.ServersMode != tc.wantServers {
				t.Fatalf("servers mode = %q, want %q", rule.ServersMode, tc.wantServers)
			}
			if len(rule.ServerIDs) != len(tc.wantIDs) {
				t.Fatalf("server ids = %v, want %v", rule.ServerIDs, tc.wantIDs)
			}
			for i := range tc.wantIDs {
				if rule.ServerIDs[i] != tc.wantIDs[i] {
					t.Fatalf("server ids = %v, want %v", rule.ServerIDs, tc.wantIDs)
				}
			}
			if rule.ToolApprovalApprovedOnly != tc.wantToolApproval {
				t.Fatalf("tool approval approved_only = %v, want %v", rule.ToolApprovalApprovedOnly, tc.wantToolApproval)
			}
		})
	}
}

func TestOrgExtensionAllowListSwitchIsTheLiteralTrue(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "1": false, "yes": false, "TRUE": false, "": false} {
		got := policy.OrgExtensionAllowListFromLookup(func(name string) string {
			if name != policy.OrgExtensionAllowListEnv {
				return ""
			}
			return value
		})
		if got != want {
			t.Fatalf("ORG_EXTENSION_ALLOWLIST=%q enables = %v, want %v", value, got, want)
		}
	}
}

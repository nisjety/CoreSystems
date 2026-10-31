package policy

import (
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

func allowBase() *Result {
	return &Result{Decision: DecisionAllow, Reason: "low-risk capability allowed under default budget", BudgetContext: "tokens=100000,cost_usd=5.00", PolicySource: PolicySourceRiskLevel}
}

// Decide only ever tightens: it adds an ask or a denial to what the capability's
// own risk decision said and never turns anything into an allow.
func TestDecideTightensButNeverLoosens(t *testing.T) {
	tests := []struct {
		name        string
		base        *Result
		binding     *ToolBinding
		wantOutcome string
		wantSource  string
		wantReason  string
	}{
		{
			name:        "no binding is a capability-level evaluation",
			base:        allowBase(),
			wantOutcome: DecisionAllow, wantSource: PolicySourceRiskLevel,
		},
		{
			name:        "a read tool with no floor keeps the base allow",
			base:        allowBase(),
			binding:     &ToolBinding{ToolName: "web_fetch", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone},
			wantOutcome: DecisionAllow, wantSource: PolicySourceRiskLevel,
		},
		{
			name:        "an ask floor turns an allow into an ask",
			base:        allowBase(),
			binding:     &ToolBinding{ToolName: "read_subagent_result", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorAsk},
			wantOutcome: DecisionAsk, wantSource: PolicySourceApprovalFloor,
		},
		{
			name:        "a durable owner floor turns an allow into an ask",
			base:        allowBase(),
			binding:     &ToolBinding{ToolName: "tickets.create", SideEffectClass: models.SideEffectOwnerEffect, ApprovalFloor: models.ApprovalFloorDurableOwner},
			wantOutcome: DecisionAsk, wantSource: PolicySourceApprovalFloor,
		},
		{
			name:        "an owner effect row stored without its floor is still an ask",
			base:        allowBase(),
			binding:     &ToolBinding{ToolName: "tickets.create", SideEffectClass: models.SideEffectOwnerEffect, ApprovalFloor: models.ApprovalFloorNone},
			wantOutcome: DecisionAsk, wantSource: PolicySourceApprovalFloor,
		},
		{
			name:        "an unclassified tool is an ask whatever its stored floor says",
			base:        allowBase(),
			binding:     &ToolBinding{ToolName: "mcp__s__t", SideEffectClass: models.SideEffectUnclassified, ApprovalFloor: models.ApprovalFloorNone, ReviewState: ReviewStateApproved},
			wantOutcome: DecisionAsk, wantSource: PolicySourceApprovalFloor,
		},
		{
			name:        "an unrecognised class is treated as unclassified",
			base:        allowBase(),
			binding:     &ToolBinding{ToolName: "t", SideEffectClass: "read-ish", ApprovalFloor: models.ApprovalFloorNone},
			wantOutcome: DecisionAsk, wantSource: PolicySourceApprovalFloor,
		},
		{
			name:        "an empty floor is the strictest floor",
			base:        allowBase(),
			binding:     &ToolBinding{ToolName: "t", SideEffectClass: models.SideEffectRead, ApprovalFloor: ""},
			wantOutcome: DecisionAsk, wantSource: PolicySourceApprovalFloor,
		},
		{
			name:        "a base ask is never relaxed by a read tool",
			base:        &Result{Decision: DecisionAsk, Reason: "high-risk", PolicySource: PolicySourceRiskLevel},
			binding:     &ToolBinding{ToolName: "web_fetch", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone},
			wantOutcome: DecisionAsk, wantSource: PolicySourceRiskLevel, wantReason: "high-risk",
		},
		{
			name:        "a base denial stands over a floor",
			base:        &Result{Decision: DecisionDeny, Reason: "scope_not_enabled", PolicySource: PolicySourceCapabilityGate},
			binding:     &ToolBinding{ToolName: "t", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorAsk},
			wantOutcome: DecisionDeny, wantSource: PolicySourceCapabilityGate, wantReason: "scope_not_enabled",
		},
		{
			name:        "a base denial stands over a review state",
			base:        &Result{Decision: DecisionDeny, Reason: "scope_not_enabled", PolicySource: PolicySourceCapabilityGate},
			binding:     &ToolBinding{ToolName: "t", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone, ReviewState: ReviewStateDrifted},
			wantOutcome: DecisionDeny, wantSource: PolicySourceCapabilityGate, wantReason: "scope_not_enabled",
		},
		{
			name:        "an approved MCP read keeps the base allow",
			base:        allowBase(),
			binding:     &ToolBinding{ToolName: "mcp__s__echo", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone, ReviewState: ReviewStateApproved, ToolDigest: strings.Repeat("a", 64)},
			wantOutcome: DecisionAllow, wantSource: PolicySourceRiskLevel,
		},
		{
			name:        "a drifted tool is denied with its own cause",
			base:        allowBase(),
			binding:     &ToolBinding{ToolName: "mcp__s__echo", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone, ReviewState: ReviewStateDrifted},
			wantOutcome: DecisionDeny, wantSource: PolicySourceToolReview, wantReason: "tool_definition_changed",
		},
		{
			name:        "a pending tool is denied as not approved",
			base:        allowBase(),
			binding:     &ToolBinding{ToolName: "mcp__s__echo", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone, ReviewState: "pending"},
			wantOutcome: DecisionDeny, wantSource: PolicySourceToolReview, wantReason: "tool_not_approved",
		},
		{
			name:        "an unknown review state is denied, never treated as approved",
			base:        allowBase(),
			binding:     &ToolBinding{ToolName: "mcp__s__echo", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone, ReviewState: "approved "},
			wantOutcome: DecisionDeny, wantSource: PolicySourceToolReview, wantReason: "tool_not_approved",
		},
		{
			name:        "a base ask on a drifted tool is still denied",
			base:        &Result{Decision: DecisionAsk, Reason: "high-risk", PolicySource: PolicySourceRiskLevel},
			binding:     &ToolBinding{ToolName: "mcp__s__echo", SideEffectClass: models.SideEffectExternalWrite, ApprovalFloor: models.ApprovalFloorNone, ReviewState: ReviewStateDrifted},
			wantOutcome: DecisionDeny, wantSource: PolicySourceToolReview, wantReason: "tool_definition_changed",
		},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := Decide(tc.base, tc.binding)
			if got.Decision != tc.wantOutcome {
				t.Fatalf("decision = %q, want %q (%+v)", got.Decision, tc.wantOutcome, got)
			}
			if got.PolicySource != tc.wantSource {
				t.Fatalf("policy source = %q, want %q", got.PolicySource, tc.wantSource)
			}
			if tc.wantReason != "" && got.Reason != tc.wantReason {
				t.Fatalf("reason = %q, want %q", got.Reason, tc.wantReason)
			}
			if got.Decision == DecisionAsk && got.Reason == "" {
				t.Fatal("an ask carries no reason")
			}
		})
	}
}

// Decide is pure: it never mutates the base result the caller still holds.
func TestDecideDoesNotMutateItsInputs(t *testing.T) {
	base := allowBase()
	binding := &ToolBinding{ToolName: "t", SideEffectClass: models.SideEffectUnclassified, ApprovalFloor: models.ApprovalFloorNone}
	_ = Decide(base, binding)
	if *base != *allowBase() {
		t.Fatalf("base was changed: %+v", base)
	}
	if binding.ApprovalFloor != models.ApprovalFloorNone || binding.SideEffectClass != models.SideEffectUnclassified {
		t.Fatalf("binding was changed: %+v", binding)
	}
	if got := Decide(nil, binding); got != nil {
		t.Fatalf("a nil base produced %+v", got)
	}
}

// The ask a floor adds has no budget, so the allow's budget does not leak into a
// decision that still needs a person.
func TestDecideAskCarriesNoBudget(t *testing.T) {
	got := Decide(allowBase(), &ToolBinding{ToolName: "t", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorAsk})
	if got.BudgetContext != "" {
		t.Fatalf("an ask kept the allow's budget: %q", got.BudgetContext)
	}
}

// A tool the registry holds no row for is held to its capability's ceiling, so a
// low-risk capability whose ceiling is an owner effect or an ask floor can never
// be decided as a plain allow for a tool nobody declared.
func TestCeilingBindingHoldsAnUndeclaredToolToTheCapabilitysFloor(t *testing.T) {
	for _, tc := range []struct {
		name       string
		capability *models.Capability
		want       string
		wantSource string
		wantFloor  string
	}{
		{"an owner effect ceiling asks", &models.Capability{ID: "cap.tool.ticket.create", RiskLevel: models.RiskLow, SideEffectClass: models.SideEffectOwnerEffect, ApprovalFloor: models.ApprovalFloorDurableOwner}, DecisionAsk, PolicySourceApprovalFloor, models.ApprovalFloorDurableOwner},
		{"an owner effect ceiling stored without its floor still asks", &models.Capability{ID: "cap.tool.ticket.create", RiskLevel: models.RiskLow, SideEffectClass: models.SideEffectOwnerEffect}, DecisionAsk, PolicySourceApprovalFloor, models.ApprovalFloorDurableOwner},
		{"an ask floor ceiling asks", &models.Capability{ID: "cap.agent.lineage.read", RiskLevel: models.RiskLow, SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorAsk}, DecisionAsk, PolicySourceApprovalFloor, models.ApprovalFloorAsk},
		{"an unclassified ceiling with no floor stored takes the strictest floor", &models.Capability{ID: "cap.x", RiskLevel: models.RiskLow}, DecisionAsk, PolicySourceApprovalFloor, models.ApprovalFloorDurableOwner},
		{"a read ceiling with no floor is a plain allow", &models.Capability{ID: "cap.tool.http", RiskLevel: models.RiskLow, SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone}, DecisionAllow, PolicySourceRiskLevel, models.ApprovalFloorNone},
	} {
		t.Run(tc.name, func(t *testing.T) {
			binding := CeilingBinding(tc.capability, "some_tool")
			if binding == nil || binding.ToolName != "some_tool" || binding.CapabilityID != tc.capability.ID {
				t.Fatalf("binding = %+v", binding)
			}
			if binding.ReviewState != "" || binding.ToolDigest != "" {
				t.Fatalf("a ceiling is not a registry row, it has no review state or digest: %+v", binding)
			}
			if binding.ApprovalFloor != tc.wantFloor {
				t.Fatalf("floor = %q, want %q", binding.ApprovalFloor, tc.wantFloor)
			}
			got := Decide(riskResult(tc.capability), binding)
			if got.Decision != tc.want || got.PolicySource != tc.wantSource {
				t.Fatalf("decision = %q from %q, want %q from %q (%+v)", got.Decision, got.PolicySource, tc.want, tc.wantSource, got)
			}
			// The same capability decided with no binding at all is the bug being
			// closed: the risk level alone allows it.
			if tc.want == DecisionAsk && riskResult(tc.capability).Decision != DecisionAllow {
				t.Fatalf("the fixture must be a low-risk allow without the ceiling")
			}
		})
	}
	if CeilingBinding(nil, "t") != nil {
		t.Fatal("a nil capability has no ceiling")
	}
}

// EffectivePolicy is the capability's risk decision tightened by the tool, and
// is a denial for a capability that cannot be evaluated at all.
func TestEffectivePolicyFollowsRiskThenToolDeclaration(t *testing.T) {
	low := &models.Capability{ID: "cap.low", RiskLevel: models.RiskLow}
	high := &models.Capability{ID: "cap.high", RiskLevel: models.RiskHigh}
	invalid := &models.Capability{ID: "cap.bad", RiskLevel: "critical-ish"}
	read := &ToolBinding{ToolName: "t", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone}
	floor := &ToolBinding{ToolName: "t", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorAsk}
	drifted := &ToolBinding{ToolName: "t", SideEffectClass: models.SideEffectRead, ApprovalFloor: models.ApprovalFloorNone, ReviewState: ReviewStateDrifted}

	for _, tc := range []struct {
		name       string
		capability *models.Capability
		binding    *ToolBinding
		want       string
	}{
		{"low risk read", low, read, DecisionAllow},
		{"low risk with an ask floor", low, floor, DecisionAsk},
		{"low risk drifted", low, drifted, DecisionDeny},
		{"high risk read still asks", high, read, DecisionAsk},
		{"invalid risk denies", invalid, read, DecisionDeny},
		{"no capability denies", nil, read, DecisionDeny},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if got := EffectivePolicy(tc.capability, tc.binding); got != tc.want {
				t.Fatalf("EffectivePolicy = %q, want %q", got, tc.want)
			}
		})
	}
}

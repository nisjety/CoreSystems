// Package policy contains the capability policy engine. The default
// heuristic denies high-risk capabilities outright, allows medium-risk with a
// conservative token/cost budget, and allows low-risk with generous budgets.
// Replace with an OPA-backed or rules-engine-backed implementation once the
// control plane exposes policy bundles.
package policy

import (
	"context"
	"fmt"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
	"github.com/triodelab/model-plane/services/capability-core/internal/telemetry"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
)

// Decision values.
const (
	DecisionAllow = "allow"
	DecisionDeny  = "deny"
	DecisionAsk   = "ask"
)

// ScopeWildcard matches any scope in an EnabledForScopes entry.
const ScopeWildcard = "*"

// Policy sources name what produced a decision. They ride in the signed
// decision so a later reader can tell a risk-level ask from a class-floor ask
// without re-deriving it.
const (
	// PolicySourceRiskLevel: the capability's own risk level decided.
	PolicySourceRiskLevel = "risk_level"
	// PolicySourceApprovalFloor: a tool's class or approval floor raised an
	// allow to an ask.
	PolicySourceApprovalFloor = "approval_floor"
	// PolicySourceToolReview: the tool is not approved, or the registry does not
	// hold it.
	PolicySourceToolReview = "tool_review"
	// PolicySourceCapabilityGate: the capability is out of rollout, unavailable,
	// outside the requested scope or ungranted.
	PolicySourceCapabilityGate = "capability_gate"
)

// Review states of an MCP tool row (capability_tools.review_state). A built-in
// tool has none.
const (
	ReviewStateApproved = "approved"
	ReviewStateDrifted  = "drifted"
)

// ScopeResolver checks the durable capability_scopes grant table. It is the
// tenant-bound org invocation authority layer today: distinct from a capability's static
// EnabledForScopes array, which only declares which scope *kinds* a capability
// supports. registry.ScopeStore satisfies this interface.
//
// IsGrantedForScope reports whether an active grant owned by the verified
// tenant covers the exact (kind, value); wildcard values remain tenant-bound.
type ScopeResolver interface {
	IsGrantedForScope(ctx context.Context, capabilityID, orgID, scopeKind, scopeValue string) (bool, error)
}

// IsSupportedScope reports whether policy can derive the scope authority from
// the authenticated EvaluatePolicy tuple today. Global has no resource ID and
// org derives its value from the verified tenant. Agent, run, thread,
// workspace, and user remain persisted catalog scope kinds but are rejected at
// invocation until trusted concrete identity/resource bindings are added.
func IsSupportedScope(scope string) bool {
	switch scope {
	case registry.ScopeKindOrg,
		registry.ScopeKindGlobal:
		return true
	default:
		return false
	}
}

// Engine evaluates capability invocation requests against the registry.
//
// reg is unused by any exported method today: the two methods that used to
// read it (Evaluate, which resolved a capability by ID through the registry's
// org-blind, last-write-wins index, and Enforce, an RBAC check with the same
// org-blind lookup) were removed as POL-2 — neither had a live production
// caller (every gRPC/HTTP entry point already resolves capabilities through a
// tenant-scoped registry.GetForOrg/ListForOrg first and calls
// EvaluateCapability directly with the resolved snapshot), so they were a
// cross-tenant landmine for whoever wired them in next rather than a
// reachable vulnerability. The field is kept, rather than changing New's
// signature, so existing callers that also use reg for their own org-aware
// lookups are unaffected. See apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md.
type Engine struct {
	reg    *registry.Registry
	scopes ScopeResolver
}

// WithScopeResolver attaches a durable scope-grant resolver. Returns the same
// Engine for chaining. Nil resolver (the default) leaves grant enforcement off,
// preserving the prior static-only scope semantics.
func (e *Engine) WithScopeResolver(r ScopeResolver) *Engine {
	e.scopes = r
	return e
}

// New constructs an Engine backed by the provided registry.
func New(reg *registry.Registry) *Engine { return &Engine{reg: reg} }

// Result holds the outcome of an evaluation.
type Result struct {
	Decision      string
	Reason        string
	BudgetContext string
	// PolicySource is one of the PolicySource constants.
	PolicySource string
}

// ToolBinding is the registry's declaration of one tool, resolved by the
// caller from capability_tools. It is plain data so Decide stays pure.
type ToolBinding struct {
	ToolName          string
	CapabilityID      string
	CapabilityVersion string
	SideEffectClass   string
	ApprovalFloor     string
	// ToolDigest is the pinned definition digest of an MCP tool; empty for a
	// built-in tool.
	ToolDigest string
	// ReviewState is empty for a built-in tool.
	ReviewState string
}

// CeilingBinding is the binding a named tool is held to when the registry holds
// no row for it anywhere: the capability's own class and approval floor, which is
// never weaker than any of its tools' declarations. Without it a tool named under
// an `owner_effect` or `durable_owner` capability but absent from capability_tools
// would be decided on the capability's risk level alone and signed as a plain
// allow, with a floor in the evidence that nothing enforced. It has no review
// state and no digest: it is a ceiling, not a registry row.
func CeilingBinding(capEntry *models.Capability, toolName string) *ToolBinding {
	if capEntry == nil {
		return nil
	}
	return &ToolBinding{
		ToolName:          toolName,
		CapabilityID:      capEntry.ID,
		CapabilityVersion: capEntry.Version,
		SideEffectClass:   models.NormalizeSideEffectClass(capEntry.SideEffectClass),
		ApprovalFloor:     models.EffectiveApprovalFloor(capEntry.SideEffectClass, capEntry.ApprovalFloor),
	}
}

// Decide tightens a capability-level result with the binding of the tool being
// called. It only ever tightens: a denial stands, a tool that is not approved is
// denied, and a class or approval floor of `ask` or above turns an allow into an
// ask. The capability's own risk decision, scope and grant checks are made
// before and are not repeated here. A nil binding is a capability-level
// evaluation and returns base unchanged.
func Decide(base *Result, binding *ToolBinding) *Result {
	if base == nil || binding == nil || base.Decision == DecisionDeny {
		return base
	}
	if binding.ReviewState != "" && binding.ReviewState != ReviewStateApproved {
		reason := "tool_not_approved"
		if binding.ReviewState == ReviewStateDrifted {
			reason = "tool_definition_changed"
		}
		return &Result{Decision: DecisionDeny, Reason: reason, PolicySource: PolicySourceToolReview}
	}
	floor := models.EffectiveApprovalFloor(binding.SideEffectClass, binding.ApprovalFloor)
	if base.Decision == DecisionAllow && models.ApprovalFloorRank(floor) >= models.ApprovalFloorRank(models.ApprovalFloorAsk) {
		return &Result{
			Decision:     DecisionAsk,
			Reason:       fmt.Sprintf("tool %s is declared %s with approval floor %s and requires human approval", binding.ToolName, models.NormalizeSideEffectClass(binding.SideEffectClass), floor),
			PolicySource: PolicySourceApprovalFloor,
		}
	}
	return base
}

// EvaluateCapability evaluates a capability snapshot that the caller has
// already resolved through a tenant-aware registry lookup. Keeping lookup and
// evaluation separate lets authenticated boundaries avoid re-reading a mixed
// process-wide registry after authorization.
func (e *Engine) EvaluateCapability(ctx context.Context, capEntry *models.Capability, runID, agentID, orgID, scope string) (*Result, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if capEntry == nil || capEntry.ID == "" || runID == "" || agentID == "" || orgID == "" || !IsSupportedScope(scope) {
		return nil, domain.ErrInvalidArgument
	}
	if denial := e.denyOnScope(ctx, capEntry, scope); denial != nil {
		return denial, nil
	}
	if denial, derr := e.denyOnGrant(ctx, capEntry, agentID, orgID, scope); derr != nil {
		return nil, derr
	} else if denial != nil {
		return denial, nil
	}
	result := riskResult(capEntry)
	attrs := []attribute.KeyValue{
		attribute.String("decision", result.Decision),
		attribute.String("risk", string(capEntry.RiskLevel)),
	}
	if result.Decision == DecisionDeny {
		attrs = append(attrs, attribute.String("reason", result.Reason))
	}
	telemetry.PolicyDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attrs...))
	return result, nil
}

// riskResult is the decision a capability's risk level alone yields: high risk
// asks, medium and low allow under a budget, anything else is denied. It is the
// base every other check tightens and has no side effect.
func riskResult(capEntry *models.Capability) *Result {
	switch capEntry.RiskLevel {
	case models.RiskHigh:
		return &Result{
			Decision:      DecisionAsk,
			Reason:        fmt.Sprintf("capability %s is high-risk and requires human approval", capEntry.ID),
			BudgetContext: "",
			PolicySource:  PolicySourceRiskLevel,
		}
	case models.RiskMedium:
		return &Result{
			Decision:      DecisionAllow,
			Reason:        "medium-risk capability allowed under constrained budget",
			BudgetContext: "tokens=10000,cost_usd=0.50",
			PolicySource:  PolicySourceRiskLevel,
		}
	case models.RiskLow:
		return &Result{
			Decision:      DecisionAllow,
			Reason:        "low-risk capability allowed under default budget",
			BudgetContext: "tokens=100000,cost_usd=5.00",
			PolicySource:  PolicySourceRiskLevel,
		}
	default:
		return &Result{
			Decision:     DecisionDeny,
			Reason:       "invalid_risk_level",
			PolicySource: PolicySourceRiskLevel,
		}
	}
}

// EffectivePolicy is what a tool's own declaration lets a call do today: the
// capability's risk decision tightened by the tool's review state and approval
// floor. Scope and grant checks are per call and are not part of it, so it is
// the answer to "may this tool ever run unattended", not a replacement for
// EvaluateCapability.
func EffectivePolicy(capEntry *models.Capability, binding *ToolBinding) string {
	if capEntry == nil {
		return DecisionDeny
	}
	return Decide(riskResult(capEntry), binding).Decision
}

// denyOnScope returns a Deny Result unless the capability explicitly declares
// the invocation scope (or the wildcard).
func (e *Engine) denyOnScope(ctx context.Context, cap *models.Capability, scope string) *Result {
	for _, s := range cap.EnabledForScopes {
		if s == ScopeWildcard || s == scope {
			return nil
		}
	}
	telemetry.PolicyDecisionsTotal.Add(ctx, 1, metric.WithAttributes(
		attribute.String("decision", DecisionDeny),
		attribute.String("risk", string(cap.RiskLevel)),
		attribute.String("reason", "scope_not_enabled"),
	))
	return &Result{
		Decision: DecisionDeny,
		Reason: fmt.Sprintf(
			"capability %s not enabled for scope %q (allowed: %v)",
			cap.ID, scope, cap.EnabledForScopes,
		),
		BudgetContext: "",
		PolicySource:  PolicySourceCapabilityGate,
	}
}

// scopeValueFor resolves the concrete scope_value to look up in the grant table
// for a given scope kind from the authenticated request tuple. Only org has a
// trusted concrete value today.
// Unsupported resource kinds never reach this function because evaluation
// rejects them before capability or durable-grant checks.
func scopeValueFor(scopeKind, orgID string) string {
	switch scopeKind {
	case registry.ScopeKindOrg:
		return orgID
	default:
		return ""
	}
}

// denyOnGrant consults the durable capability_scopes grant table. It only acts
// when a ScopeResolver is configured and `scope` carries the verified org
// value. An explicit tenant-bound grant is
// mandatory; the absence of grants must not turn durable authorization off.
// Global retains its static catalog semantics. Returns (denial, nil) on a
// concrete grant miss and (nil, err) on a resolver/DB error.
func (e *Engine) denyOnGrant(ctx context.Context, cap *models.Capability, _ string, orgID, scope string) (*Result, error) {
	if e.scopes == nil || scope == "" {
		return nil, nil
	}
	scopeValue := scopeValueFor(scope, orgID)
	if scopeValue == "" {
		return nil, nil
	}
	granted, err := e.scopes.IsGrantedForScope(ctx, cap.ID, orgID, scope, scopeValue)
	if err != nil {
		return nil, err
	}
	if granted {
		return nil, nil
	}
	telemetry.PolicyDecisionsTotal.Add(ctx, 1, metric.WithAttributes(
		attribute.String("decision", DecisionDeny),
		attribute.String("risk", string(cap.RiskLevel)),
		attribute.String("reason", "scope_grant_missing"),
	))
	return &Result{
		Decision: DecisionDeny,
		Reason: fmt.Sprintf(
			"capability %s has no active %s-scope grant for %q",
			cap.ID, scope, scopeValue,
		),
		BudgetContext: "",
		PolicySource:  PolicySourceCapabilityGate,
	}, nil
}

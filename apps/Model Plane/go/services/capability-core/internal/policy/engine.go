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
	// PolicySourceToolPolicyNever, Ask and Allow: a stored per-tool policy (user or
	// org) decided. They are also the decision's reason code.
	PolicySourceToolPolicyNever = "tool_policy_never"
	PolicySourceToolPolicyAsk   = "tool_policy_ask"
	PolicySourceToolPolicyAllow = "tool_policy_allow"
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
	// toolPolicies is read only when policiesEnforced; with it off a stored policy
	// is never loaded, so it cannot change a decision.
	toolPolicies     ToolPolicySource
	policiesEnforced bool
}

// WithScopeResolver attaches a durable scope-grant resolver. Returns the same
// Engine for chaining. Nil resolver (the default) leaves grant enforcement off,
// preserving the prior static-only scope semantics.
func (e *Engine) WithScopeResolver(r ScopeResolver) *Engine {
	e.scopes = r
	return e
}

// WithToolPolicies attaches the store of per-tool policies. They are honoured
// only when enforced is true, which main wires from TOOL_POLICY_ENFORCED; a nil
// source can never be enforced.
func (e *Engine) WithToolPolicies(source ToolPolicySource, enforced bool) *Engine {
	e.toolPolicies = source
	e.policiesEnforced = enforced && source != nil
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

// ToolPolicyEnforcedEnv switches per-tool policies on. Off, they are stored and
// ignored.
const ToolPolicyEnforcedEnv = "TOOL_POLICY_ENFORCED"

// ToolPolicyEnforcedFromLookup reads ToolPolicyEnforcedEnv strictly: only the
// literal "true" enables, so a typo or a stray "1" can never switch a policy
// layer on by accident.
func ToolPolicyEnforcedFromLookup(lookup func(string) string) bool {
	return lookup(ToolPolicyEnforcedEnv) == "true"
}

// The decisions and scope kinds of a stored tool policy.
const (
	ToolPolicyAllow = "allow"
	ToolPolicyAsk   = "ask"
	ToolPolicyNever = "never"

	ToolPolicyScopeOrg  = "org"
	ToolPolicyScopeUser = "user"
)

// ToolPolicy is one active stored policy on a capability's tools.
type ToolPolicy struct {
	// ScopeKind is ToolPolicyScopeOrg or ToolPolicyScopeUser.
	ScopeKind string
	// Decision is ToolPolicyAllow, ToolPolicyAsk or ToolPolicyNever.
	Decision string
	// PinnedDigest is the tool definition an `allow` was given for; empty for a
	// tightening policy.
	PinnedDigest string
}

// ToolPolicySource loads the active policies that apply to one person's call of
// a capability: the org's own and, when userID is not empty, that person's.
type ToolPolicySource interface {
	ActiveToolPolicies(ctx context.Context, orgID, userID, capabilityID string) ([]ToolPolicy, error)
}

// CanRelax reports whether a policy of scopeKind may relax a tool of this class
// and floor to `allow`. A person may relax for themselves up to an external
// write and an org admin for the org up to a destructive tool; an owner effect,
// an unclassified tool and anything with an approval floor are never relaxed by
// anyone. An unknown scope kind relaxes nothing.
func CanRelax(scopeKind, class, floor string) bool {
	var ceiling string
	switch scopeKind {
	case ToolPolicyScopeOrg:
		ceiling = models.SideEffectDestructive
	case ToolPolicyScopeUser:
		ceiling = models.SideEffectExternalWrite
	default:
		return false
	}
	return models.SideEffectClassRank(class) <= models.SideEffectClassRank(ceiling) &&
		models.EffectiveApprovalFloor(class, floor) == models.ApprovalFloorNone
}

// relaxedBudget is the budget an `allow` policy grants to a tool whose risk
// level would otherwise ask: the medium-risk one, never the generous low one.
const relaxedBudget = "tokens=10000,cost_usd=0.50"

// DecideWithPolicies is Decide followed by the stored per-tool policies.
//
// Tightening applies to every row: any `never` denies, and any `ask` turns an
// allow into an ask, whichever scope set it, so an org `never` beats a person's
// `allow` and a person's `ask` stands under an org `allow`. A stored `ask` (or a
// decision outside the vocabulary) over a decision that is already an ask is
// reported as `tool_policy_ask` too, keeping the base reason and budget: the
// signed policy source then says a person or org asked for the pause on
// purpose, so no consumer can mistake it for the risk-level floor alone. A
// stale `allow` demoted to an ask is not such a request and leaves the base
// source as it is. Relaxing is narrower:
// an `allow` turns the risk-level ask of an `mcp_tool` row into an allow only
// when the tool's class and floor are within what the policy's scope may relax
// (CanRelax) and the policy is pinned to the definition the registry pins now. An
// `allow` for a definition that has since changed is read as an `ask`; an `allow`
// on a built-in tool, or on a class it may not relax, is ignored. A denial from
// Decide (a tool not approved, a gate) is never relaxed.
func DecideWithPolicies(base *Result, capEntry *models.Capability, binding *ToolBinding, policies []ToolPolicy) *Result {
	result := Decide(base, binding)
	if result == nil || result.Decision == DecisionDeny || len(policies) == 0 {
		return result
	}
	relaxable := capEntry != nil && capEntry.Kind == models.KindMCPTool && binding != nil
	// storedAsk is an explicit request for a pause; ask also covers a stale allow.
	var never, ask, storedAsk, allow bool
	for _, p := range policies {
		switch p.Decision {
		case ToolPolicyNever:
			never = true
		case ToolPolicyAsk:
			ask = true
			storedAsk = true
		case ToolPolicyAllow:
			if !relaxable || !CanRelax(p.ScopeKind, binding.SideEffectClass, binding.ApprovalFloor) {
				continue
			}
			if p.PinnedDigest == "" || p.PinnedDigest != binding.ToolDigest {
				ask = true
				continue
			}
			allow = true
		default:
			// A decision outside the vocabulary can only add a pause.
			ask = true
			storedAsk = true
		}
	}
	switch {
	case never:
		return &Result{Decision: DecisionDeny, Reason: PolicySourceToolPolicyNever, PolicySource: PolicySourceToolPolicyNever}
	case ask:
		if result.Decision == DecisionAllow {
			return &Result{Decision: DecisionAsk, Reason: PolicySourceToolPolicyAsk, PolicySource: PolicySourceToolPolicyAsk}
		}
		if storedAsk && result.Decision == DecisionAsk {
			tagged := *result
			tagged.PolicySource = PolicySourceToolPolicyAsk
			return &tagged
		}
	case allow:
		if result.Decision == DecisionAsk && result.PolicySource == PolicySourceRiskLevel {
			return &Result{Decision: DecisionAllow, Reason: PolicySourceToolPolicyAllow, BudgetContext: relaxedBudget, PolicySource: PolicySourceToolPolicyAllow}
		}
	}
	return result
}

// EvaluateCapability evaluates a capability snapshot that the caller has
// already resolved through a tenant-aware registry lookup. Keeping lookup and
// evaluation separate lets authenticated boundaries avoid re-reading a mixed
// process-wide registry after authorization.
func (e *Engine) EvaluateCapability(ctx context.Context, capEntry *models.Capability, runID, agentID, orgID, scope string) (*Result, error) {
	denial, err := e.gate(ctx, capEntry, runID, agentID, orgID, scope)
	if err != nil || denial != nil {
		return denial, err
	}
	result := riskResult(capEntry)
	recordDecision(ctx, capEntry, result, capEntry.SideEffectClass)
	return result, nil
}

// EvaluateTool is EvaluateCapability for one tool of the capability: the same
// request and gate checks, then the capability's risk decision tightened by the
// tool's registry binding (Decide) and, when TOOL_POLICY_ENFORCED is on, by the
// stored policies of the org and of userID (DecideWithPolicies). A nil binding is
// a capability-level evaluation, which the policies can still tighten. The
// policies are read after the gate and only when enforced; if they cannot be read
// the evaluation fails rather than deciding without them.
func (e *Engine) EvaluateTool(ctx context.Context, capEntry *models.Capability, binding *ToolBinding, userID, runID, agentID, orgID, scope string) (*Result, error) {
	denial, err := e.gate(ctx, capEntry, runID, agentID, orgID, scope)
	if err != nil || denial != nil {
		return denial, err
	}
	var policies []ToolPolicy
	if e.policiesEnforced {
		if policies, err = e.toolPolicies.ActiveToolPolicies(ctx, orgID, userID, capEntry.ID); err != nil {
			return nil, fmt.Errorf("load tool policies: %w", err)
		}
	}
	result := DecideWithPolicies(riskResult(capEntry), capEntry, binding, policies)
	class := capEntry.SideEffectClass
	if binding != nil {
		class = binding.SideEffectClass
	}
	recordDecision(ctx, capEntry, result, class)
	return result, nil
}

// gate validates the request and runs the checks every evaluation shares: the
// capability's declared scopes and the durable scope grant. It returns the denial
// (already counted) or nil when the capability may be evaluated for this scope.
func (e *Engine) gate(ctx context.Context, capEntry *models.Capability, runID, agentID, orgID, scope string) (*Result, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if capEntry == nil || capEntry.ID == "" || runID == "" || agentID == "" || orgID == "" || !IsSupportedScope(scope) {
		return nil, domain.ErrInvalidArgument
	}
	if denial := e.denyOnScope(ctx, capEntry, scope); denial != nil {
		return denial, nil
	}
	denial, err := e.denyOnGrant(ctx, capEntry, agentID, orgID, scope)
	if err != nil {
		return nil, err
	}
	return denial, nil
}

// recordDecision counts one evaluation outcome with the class it was made for and
// what produced it.
func recordDecision(ctx context.Context, capEntry *models.Capability, result *Result, class string) {
	attrs := []attribute.KeyValue{
		attribute.String("decision", result.Decision),
		attribute.String("risk", string(capEntry.RiskLevel)),
		attribute.String("class", models.NormalizeSideEffectClass(class)),
		attribute.String("policy_source", result.PolicySource),
	}
	if result.Decision == DecisionDeny {
		attrs = append(attrs, attribute.String("reason", result.Reason))
	}
	telemetry.PolicyDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attrs...))
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

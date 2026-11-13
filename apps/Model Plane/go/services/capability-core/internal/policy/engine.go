// Package policy contains the capability policy engine. The default
// heuristic denies high-risk capabilities outright, allows medium-risk with a
// conservative token/cost budget, and allows low-risk with generous budgets.
// Replace with an OPA-backed or rules-engine-backed implementation once the
// control plane exposes policy bundles.
package policy

import (
	"context"
	"fmt"
	"slices"
	"strings"
	"time"

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
	// PolicySourceOrgPermissionRule: an org permission rule (W5-074) tightened the decision. Its reason names the rule
	// as org_rule:<rule id> and nothing else.
	PolicySourceOrgPermissionRule = "org_permission_rule"
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
	toolPolicies                 ToolPolicySource
	policiesEnforced             bool
	browserAgentPoliciesEnforced bool
	// orgExtensions is read only when orgExtensionsEnforced; with it off the org's policy is never loaded.
	orgExtensions         OrgExtensionSource
	orgExtensionsEnforced bool
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

// WithBrowserAgentPolicyEnforced narrows policy rollout to the built-in browser
// agent without changing other tools of cap.browser.open or any other capability.
// A missing policy store fails the selected evaluation closed.
func (e *Engine) WithBrowserAgentPolicyEnforced(enforced bool) *Engine {
	e.browserAgentPoliciesEnforced = enforced
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
	// ToolPolicyChecked is signed into proof only after the store was read.
	ToolPolicyChecked bool
	// RulePolicyID is the id of the stored allow that relaxed this decision. It is set only on a
	// PolicySourceToolPolicyAllow result and is never signed into the decision proof; the server
	// reads it to write the content-free rule-use audit row (WS13 S7).
	RulePolicyID string
	// RuleScope is the scope kind (ToolPolicyScopeOrg or ToolPolicyScopeUser) of that allow; set
	// together with RulePolicyID.
	RuleScope string
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
	// ServerID is the MCP server the tool belongs to. Empty for a built-in tool, and for a binding the
	// caller could not resolve to a server, which an approved_only allow-list refuses.
	ServerID string
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

// ToolPolicyRuleAuditEnv switches the audit of rule-relaxed calls on (WS13 S7, K3-10): while it is on, a call
// relaxed by a stored allow on a persistent run writes a content-free tool_policy_rule_used row, and a run
// that does not state a persistent posture counts as retention-bound.
const ToolPolicyRuleAuditEnv = "TOOL_POLICY_RULE_AUDIT_ENABLED"

// ToolPolicyRuleAuditFromLookup reads ToolPolicyRuleAuditEnv strictly: only the literal "true" enables, with
// the semantics of ToolPolicyEnforcedFromLookup.
func ToolPolicyRuleAuditFromLookup(lookup func(string) string) bool {
	return lookup(ToolPolicyRuleAuditEnv) == "true"
}

// RetentionBoundFor reports whether a call must be treated as retention-bound, so that a person's stored
// allow does not relax it (D-WS13-1). An explicit "retention_bound" always keeps a person's rule off the
// call, whatever the audit switch; an unstated or unknown posture counts as bound only while the audit
// switch is on, so a caller that does not state it keeps today's behaviour with the switch off. An
// explicit "persistent" is never bound.
func RetentionBoundFor(runRetention string, ruleAudit bool) bool {
	return runRetention == "retention_bound" || (ruleAudit && runRetention != "persistent")
}

const BrowserAgentPolicyEnforcedEnv = "BROWSER_AGENT_POLICY_ENFORCED"

// BrowserAgentPolicyEnforcedFromLookup opts in only on the literal true.
func BrowserAgentPolicyEnforcedFromLookup(lookup func(string) string) bool {
	return lookup(BrowserAgentPolicyEnforcedEnv) == "true"
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
	// ID is the stored policy's id; it names the rule in the rule-use audit row.
	ID string
	// ScopeKind is ToolPolicyScopeOrg or ToolPolicyScopeUser.
	ScopeKind string
	// Decision is ToolPolicyAllow, ToolPolicyAsk or ToolPolicyNever.
	Decision string
	// PinnedDigest is the tool definition an `allow` was given for; empty for a
	// tightening policy.
	PinnedDigest string
	// ExpiresAt is when an `allow` lapses (build plan WS13 S7); zero for a policy
	// that does not lapse.
	ExpiresAt time.Time
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
// (CanRelax) and the policy is pinned to the definition the registry pins now and has not lapsed.
// An `allow` for a definition that has since changed, or that has lapsed, is read as an `ask`; an `allow`
// on a built-in tool, or on a class it may not relax, is ignored. A denial from
// Decide (a tool not approved, a gate) is never relaxed.
func DecideWithPolicies(base *Result, capEntry *models.Capability, binding *ToolBinding, policies []ToolPolicy) *Result {
	return DecideWithPoliciesFor(base, capEntry, binding, policies, DecideOptions{})
}

// DecideOptions are the facts about the call that DecideWithPoliciesFor needs beyond the policies.
type DecideOptions struct {
	// RetentionBound is true for a call on a run that stores no content (zero-retention or waived, DEC-32),
	// or that did not state its posture while the rule audit is on. A person's stored allow is not applied
	// to such a call, because its use could not be audited; an org allow still applies (D-WS13-1).
	RetentionBound bool
}

// DecideWithPoliciesFor is DecideWithPolicies for one call: with opts.RetentionBound a user-scope allow is
// skipped, so the base decision stands (a risk-level ask stays an ask). When a stored allow relaxes the
// decision, the result names it in RulePolicyID and RuleScope: the first applicable user-scope allow, else
// the first applicable org-scope allow.
func DecideWithPoliciesFor(base *Result, capEntry *models.Capability, binding *ToolBinding, policies []ToolPolicy, opts DecideOptions) *Result {
	result := Decide(base, binding)
	if result == nil || result.Decision == DecisionDeny || len(policies) == 0 {
		return result
	}
	relaxable := capEntry != nil && capEntry.Kind == models.KindMCPTool && binding != nil
	// storedAsk is an explicit request for a pause; ask also covers a stale allow.
	var never, ask, storedAsk, allow bool
	// userAllow and orgAllow remember the first applicable allow of each scope, for the rule-use audit.
	var userAllow, orgAllow *ToolPolicy
	for i := range policies {
		p := policies[i]
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
			// A person's rule never relaxes a retention-bound run: the use could not be audited.
			if opts.RetentionBound && p.ScopeKind == ToolPolicyScopeUser {
				continue
			}
			if p.PinnedDigest == "" || p.PinnedDigest != binding.ToolDigest || lapsed(p, time.Now()) {
				ask = true
				continue
			}
			allow = true
			switch {
			case p.ScopeKind == ToolPolicyScopeUser && userAllow == nil:
				userAllow = &policies[i]
			case p.ScopeKind == ToolPolicyScopeOrg && orgAllow == nil:
				orgAllow = &policies[i]
			}
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
			relaxed := &Result{Decision: DecisionAllow, Reason: PolicySourceToolPolicyAllow, BudgetContext: relaxedBudget, PolicySource: PolicySourceToolPolicyAllow}
			if used := firstNonNil(userAllow, orgAllow); used != nil {
				relaxed.RulePolicyID = used.ID
				relaxed.RuleScope = used.ScopeKind
			}
			return relaxed
		}
	}
	return result
}

// firstNonNil returns the first of the policies that is not nil.
func firstNonNil(policies ...*ToolPolicy) *ToolPolicy {
	for _, p := range policies {
		if p != nil {
			return p
		}
	}
	return nil
}

// lapsed reports whether a stored allow has passed its expiry (build plan WS13 S7, K3-10). A lapsed
// allow is read as a stale one, an ask and never a relaxation; the person renews it by confirming
// the rule again. A policy with no expiry never lapses.
func lapsed(p ToolPolicy, now time.Time) bool {
	return !p.ExpiresAt.IsZero() && !now.Before(p.ExpiresAt)
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
	if denial, err := e.denyOnOrgExtension(ctx, capEntry, nil, orgID); err != nil || denial != nil {
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
	return e.EvaluateToolFor(ctx, capEntry, binding, ToolCall{UserID: userID, RunID: runID, AgentID: agentID, OrgID: orgID, Scope: scope})
}

// ToolCall is one tool call to evaluate: who it acts for, which run and agent, the org and scope, and
// whether the run is retention-bound (see DecideOptions).
type ToolCall struct {
	UserID         string
	RunID          string
	AgentID        string
	OrgID          string
	Scope          string
	RetentionBound bool
}

// EvaluateToolFor is EvaluateTool for a call that states whether its run is retention-bound. With
// RetentionBound a person's stored allow is not applied to the call (DecideWithPoliciesFor).
func (e *Engine) EvaluateToolFor(ctx context.Context, capEntry *models.Capability, binding *ToolBinding, call ToolCall) (*Result, error) {
	userID, runID, agentID, orgID, scope := call.UserID, call.RunID, call.AgentID, call.OrgID, call.Scope
	denial, err := e.gate(ctx, capEntry, runID, agentID, orgID, scope)
	if err != nil || denial != nil {
		return denial, err
	}
	if denial, err := e.denyOnOrgExtension(ctx, capEntry, binding, orgID); err != nil || denial != nil {
		return denial, err
	}
	var policies []ToolPolicy
	// Do not use the capability id alone: browser.console shares this capability
	// and must retain its existing behavior while this scoped rollout is enabled.
	browserAgent := binding != nil && binding.ToolName == "browser_agent" && capEntry.ID == "cap.browser.open"
	if e.browserAgentPoliciesEnforced && browserAgent && strings.TrimSpace(userID) == "" {
		return nil, fmt.Errorf("browser agent policy requires the run owner")
	}
	policyChecked := e.policiesEnforced || (e.browserAgentPoliciesEnforced && browserAgent)
	if policyChecked {
		if e.toolPolicies == nil {
			return nil, fmt.Errorf("load tool policies: store unavailable")
		}
		if policies, err = e.toolPolicies.ActiveToolPolicies(ctx, orgID, userID, capEntry.ID); err != nil {
			return nil, fmt.Errorf("load tool policies: %w", err)
		}
	}
	result := DecideWithPoliciesFor(riskResult(capEntry), capEntry, binding, policies, DecideOptions{RetentionBound: call.RetentionBound})
	result.ToolPolicyChecked = policyChecked
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

// OrgExtensionAllowListEnv switches the org's extension allow-list on at the signed gate (WS25-S9). Off by default:
// with it off the gate never reads the org's policy, so every decision is what it was before the registry existed.
const OrgExtensionAllowListEnv = "ORG_EXTENSION_ALLOWLIST"

// OrgExtensionAllowListFromLookup reads OrgExtensionAllowListEnv strictly: only the literal "true" enables it.
func OrgExtensionAllowListFromLookup(lookup func(string) string) bool {
	return lookup(OrgExtensionAllowListEnv) == "true"
}

// ReasonOrgExtensionNotAllowed is the reason an MCP server the org has not approved is denied with. The org's policy
// names it and the gate enforces it, so a gateway that offered the server anyway could not run its tools.
const ReasonOrgExtensionNotAllowed = "org_extension_not_allowed"

// The modes of the org's MCP server allow-list (ext.mcp_servers in the org agent policy schema).
const (
	OrgServersAny          = "any"
	OrgServersApprovedOnly = "approved_only"
	OrgServersNone         = "none"
)

// OrgExtensionRule is the part of an org's agent policy the signed gate enforces for MCP servers and tools (WS25-S9).
type OrgExtensionRule struct {
	// ServersMode is OrgServersAny, OrgServersApprovedOnly or OrgServersNone.
	ServersMode string
	// ServerIDs are the servers an approved_only allow-list names.
	ServerIDs []string
	// ToolApprovalApprovedOnly offers an MCP tool only with a pinned definition (mcp_tool_approval).
	ToolApprovalApprovedOnly bool
}

// OrgExtensionRuleFromSettings reads the rule from an org agent policy document. The lockdown bundle allows no MCP
// server. A value this build cannot read narrows, as the schema's fail-closed values do: an allow-list it cannot read
// allows nothing, and a tool-approval mode it cannot read requires approval. A key the document omits takes its default.
func OrgExtensionRuleFromSettings(settings map[string]any) OrgExtensionRule {
	rule := OrgExtensionRule{ServersMode: OrgServersAny}
	if raw, present := settings["ext.mcp_servers"]; present {
		rule.ServersMode, rule.ServerIDs = readOrgServersAllowList(raw)
	}
	if lockdown, _ := settings["lockdown"].(bool); lockdown {
		rule.ServersMode, rule.ServerIDs = OrgServersNone, nil
	}
	if raw, present := settings["mcp_tool_approval"]; present {
		approval, isString := raw.(string)
		rule.ToolApprovalApprovedOnly = !isString || approval != "trust_annotations"
	}
	return rule
}

// readOrgServersAllowList reads {"mode": ..., "ids": [...]}. A shape it cannot read allows no server.
func readOrgServersAllowList(raw any) (string, []string) {
	allowList, isMap := raw.(map[string]any)
	if !isMap {
		return OrgServersNone, nil
	}
	mode, _ := allowList["mode"].(string)
	switch mode {
	case OrgServersAny:
		return OrgServersAny, nil
	case OrgServersApprovedOnly:
		rawIDs, _ := allowList["ids"].([]any)
		ids := make([]string, 0, len(rawIDs))
		for _, rawID := range rawIDs {
			if id, isString := rawID.(string); isString && id != "" {
				ids = append(ids, id)
			}
		}
		return OrgServersApprovedOnly, ids
	default:
		return OrgServersNone, nil
	}
}

// AllowsServer reports whether the rule lets a call reach the MCP server. An approved_only rule names servers by id, so
// a call whose server is not known allows nothing.
func (r OrgExtensionRule) AllowsServer(serverID string) bool {
	switch r.ServersMode {
	case OrgServersAny:
		return true
	case OrgServersApprovedOnly:
		return serverID != "" && slices.Contains(r.ServerIDs, serverID)
	default:
		return false
	}
}

// OrgExtensionSource reads the org's extension rule for an evaluation. An error fails the evaluation: the gate never
// decides an MCP call without knowing what the org allows.
type OrgExtensionSource interface {
	OrgExtensionRule(ctx context.Context, orgID string) (OrgExtensionRule, error)
}

// WithOrgExtensionPolicy attaches the org's extension rule. It is enforced only when enabled is true, which main wires
// from ORG_EXTENSION_ALLOWLIST; a nil source can never be enforced.
func (e *Engine) WithOrgExtensionPolicy(source OrgExtensionSource, enabled bool) *Engine {
	e.orgExtensions = source
	e.orgExtensionsEnforced = enabled && source != nil
	return e
}

// denyOnOrgExtension refuses an MCP server or tool the org's extension rule does not allow. A built-in tool is not an
// extension and is never held to the rule. It returns (denial, nil) on a refusal and (nil, err) when the rule cannot be
// read.
func (e *Engine) denyOnOrgExtension(ctx context.Context, capEntry *models.Capability, binding *ToolBinding, orgID string) (*Result, error) {
	if !e.orgExtensionsEnforced || capEntry.Kind != models.KindMCPTool {
		return nil, nil
	}
	rule, err := e.orgExtensions.OrgExtensionRule(ctx, orgID)
	if err != nil {
		return nil, fmt.Errorf("load org extension rule: %w", err)
	}
	serverID := ""
	if binding != nil {
		serverID = binding.ServerID
	}
	if !rule.AllowsServer(serverID) {
		return orgExtensionDenial(ctx, capEntry, ReasonOrgExtensionNotAllowed), nil
	}
	if rule.ToolApprovalApprovedOnly && (binding == nil || binding.ToolDigest == "") {
		return orgExtensionDenial(ctx, capEntry, models.ReasonToolNotApproved), nil
	}
	return nil, nil
}

// orgExtensionDenial is a refusal under an org's extension rule, counted as every other denial is.
func orgExtensionDenial(ctx context.Context, capEntry *models.Capability, reason string) *Result {
	telemetry.PolicyDecisionsTotal.Add(ctx, 1, metric.WithAttributes(
		attribute.String("decision", DecisionDeny),
		attribute.String("risk", string(capEntry.RiskLevel)),
		attribute.String("reason", reason),
	))
	return &Result{Decision: DecisionDeny, Reason: reason, PolicySource: PolicySourceCapabilityGate}
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

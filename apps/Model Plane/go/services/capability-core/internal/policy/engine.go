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
)

// ScopeWildcard matches any scope in an EnabledForScopes entry.
const ScopeWildcard = "*"

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
type Engine struct {
	reg          *registry.Registry
	subjectRoles map[string][]string
	roleCaps     map[string][]string
	scopes       ScopeResolver
}

// WithScopeResolver attaches a durable scope-grant resolver. Returns the same
// Engine for chaining. Nil resolver (the default) leaves grant enforcement off,
// preserving the prior static-only scope semantics.
func (e *Engine) WithScopeResolver(r ScopeResolver) *Engine {
	e.scopes = r
	return e
}

// NewWithRBAC constructs an Engine with RBAC bindings for Enforce.
func NewWithRBAC(reg *registry.Registry, subjectRoles, roleCaps map[string][]string) *Engine {
	return &Engine{reg: reg, subjectRoles: subjectRoles, roleCaps: roleCaps}
}

// Enforce authorizes a subject to invoke a capability based on RBAC bindings.
func (e *Engine) Enforce(ctx context.Context, subject, capID string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if subject == "" || capID == "" {
		return domain.ErrInvalidArgument
	}
	if _, err := e.reg.Get(capID, ""); err != nil {
		return err
	}
	for _, role := range e.subjectRoles[subject] {
		for _, allowed := range e.roleCaps[role] {
			if allowed == capID {
				return nil
			}
		}
	}
	return domain.ErrPermissionDenied
}

// New constructs an Engine backed by the provided registry.
func New(reg *registry.Registry) *Engine { return &Engine{reg: reg} }

// Result holds the outcome of an evaluation.
type Result struct {
	Decision      string
	Reason        string
	BudgetContext string
}

// Evaluate returns a Result for the (capability, run, agent, org, scope) tuple.
// The engine never returns ErrPolicyDenied — denials are modelled as a
// populated Result. Errors are only surfaced for argument or lookup failures.
//
// Scope is mandatory and must currently be global or org. The
// capability must explicitly authorize the kind through EnabledForScopes (or
// the wildcard), so omission and resource scopes without trusted IDs fail
// closed at both evaluation boundaries.
func (e *Engine) Evaluate(ctx context.Context, capID, runID, agentID, orgID, scope string) (*Result, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if capID == "" || runID == "" || agentID == "" || orgID == "" || !IsSupportedScope(scope) {
		return nil, domain.ErrInvalidArgument
	}
	capEntry, err := e.reg.Get(capID, "")
	if err != nil {
		return nil, err
	}
	return e.EvaluateCapability(ctx, capEntry, runID, agentID, orgID, scope)
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
	switch capEntry.RiskLevel {
	case models.RiskHigh:
		telemetry.PolicyDecisionsTotal.Add(ctx, 1, metric.WithAttributes(
			attribute.String("decision", DecisionDeny),
			attribute.String("risk", string(capEntry.RiskLevel)),
		))
		return &Result{
			Decision:      DecisionDeny,
			Reason:        fmt.Sprintf("capability %s is high-risk and requires human approval", capEntry.ID),
			BudgetContext: "",
		}, nil
	case models.RiskMedium:
		telemetry.PolicyDecisionsTotal.Add(ctx, 1, metric.WithAttributes(
			attribute.String("decision", DecisionAllow),
			attribute.String("risk", string(capEntry.RiskLevel)),
		))
		return &Result{
			Decision:      DecisionAllow,
			Reason:        "medium-risk capability allowed under constrained budget",
			BudgetContext: "tokens=10000,cost_usd=0.50",
		}, nil
	case models.RiskLow:
		telemetry.PolicyDecisionsTotal.Add(ctx, 1, metric.WithAttributes(
			attribute.String("decision", DecisionAllow),
			attribute.String("risk", string(capEntry.RiskLevel)),
		))
		return &Result{
			Decision:      DecisionAllow,
			Reason:        "low-risk capability allowed under default budget",
			BudgetContext: "tokens=100000,cost_usd=5.00",
		}, nil
	default:
		telemetry.PolicyDecisionsTotal.Add(ctx, 1, metric.WithAttributes(
			attribute.String("decision", DecisionDeny),
			attribute.String("risk", string(capEntry.RiskLevel)),
			attribute.String("reason", "invalid_risk_level"),
		))
		return &Result{
			Decision: DecisionDeny,
			Reason:   "invalid_risk_level",
		}, nil
	}
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
	}, nil
}

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
// per-(org, agent) authority layer: distinct from a capability's static
// EnabledForScopes array, which only declares which scope *kinds* a capability
// supports. registry.ScopeStore satisfies this interface.
//
// HasAnyGrants reports whether the capability is governed by the grant table
// for the given scope kind at all (no grants of that kind => not opted in, so
// the engine falls back to the static EnabledForScopes check). IsGrantedForScope
// reports whether an active grant covers the exact (kind, value) — wildcard '*'
// grants cover any value.
type ScopeResolver interface {
	HasAnyGrants(ctx context.Context, capabilityID, scopeKind string) (bool, error)
	IsGrantedForScope(ctx context.Context, capabilityID, scopeKind, scopeValue string) (bool, error)
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
// Scope enforcement (Phase 3 nice-to-have): when `scope` is non-empty and the
// capability declares a non-empty `EnabledForScopes` list, the engine denies if
// `scope` is not a member of the list. The wildcard entry "*" matches any
// scope. Empty `scope` preserves backwards compatibility — callers that have
// not yet been updated to propagate scope keep the prior allow/deny semantics.
func (e *Engine) Evaluate(ctx context.Context, capID, runID, agentID, orgID, scope string) (*Result, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if capID == "" || runID == "" || agentID == "" || orgID == "" {
		return nil, domain.ErrInvalidArgument
	}
	capEntry, err := e.reg.Get(capID, "")
	if err != nil {
		return nil, err
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
	default:
		telemetry.PolicyDecisionsTotal.Add(ctx, 1, metric.WithAttributes(
			attribute.String("decision", DecisionAllow),
			attribute.String("risk", string(capEntry.RiskLevel)),
		))
		return &Result{
			Decision:      DecisionAllow,
			Reason:        "low-risk capability allowed under default budget",
			BudgetContext: "tokens=100000,cost_usd=5.00",
		}, nil
	}
}

// denyOnScope returns a Deny Result when `scope` is non-empty, the capability
// declares a non-empty EnabledForScopes list, and that list does not contain
// `scope` (and contains no wildcard). Otherwise returns nil (scope check passes
// or is skipped).
func (e *Engine) denyOnScope(ctx context.Context, cap *models.Capability, scope string) *Result {
	if scope == "" || len(cap.EnabledForScopes) == 0 {
		return nil
	}
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
// for a given scope kind from the request tuple. org → orgID, agent → agentID.
// Kinds whose value is not carried in this tuple (run/thread/workspace/user)
// return "" — the grant check is skipped for them (they fall back to the static
// EnabledForScopes check already applied by denyOnScope).
func scopeValueFor(scopeKind, agentID, orgID string) string {
	switch scopeKind {
	case registry.ScopeKindOrg:
		return orgID
	case registry.ScopeKindAgent:
		return agentID
	default:
		return ""
	}
}

// denyOnGrant consults the durable capability_scopes grant table. It only acts
// when a ScopeResolver is configured and `scope` is one of the kinds whose
// value this tuple carries (org/agent). If the capability has *any* active
// grant of that kind, the grant table governs it: a request whose resolved
// scope_value is not covered (exact or wildcard) is denied. If the capability
// has no grants of that kind, the table is not opted-in for it and the engine
// defers to the static EnabledForScopes decision (returns nil). Returns
// (nil, nil) when the check does not apply, (denial, nil) on a grant miss, and
// (nil, err) only on a resolver/DB error.
func (e *Engine) denyOnGrant(ctx context.Context, cap *models.Capability, agentID, orgID, scope string) (*Result, error) {
	if e.scopes == nil || scope == "" {
		return nil, nil
	}
	scopeValue := scopeValueFor(scope, agentID, orgID)
	if scopeValue == "" {
		return nil, nil
	}
	governed, err := e.scopes.HasAnyGrants(ctx, cap.ID, scope)
	if err != nil {
		return nil, err
	}
	if !governed {
		return nil, nil
	}
	granted, err := e.scopes.IsGrantedForScope(ctx, cap.ID, scope, scopeValue)
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

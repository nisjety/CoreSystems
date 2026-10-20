package grant

import (
	"errors"
	"strings"
)

var (
	// ErrInvalidBudget means a budget claim is malformed: a negative bound, a
	// source that is not one of BudgetSources, a source with nothing to bound
	// or a bound with no source, or a minted-at stamp supplied by a caller.
	ErrInvalidBudget = errors.New("invalid browser grant budget claim")
	// ErrDeriveWidensBudget means a derived run grant asked for a bound above
	// the parent's, or tried to drop a bound the parent holds. A derive can
	// only tighten.
	ErrDeriveWidensBudget = errors.New("derived browser grant would widen its parent's budget claim")
)

// Budget claim sources (WS6.E). The broker stores the label verbatim and does
// not interpret it; it says who set the numbers.
const (
	// BudgetSourceRequest: the run's own request set the bound.
	BudgetSourceRequest = "request"
	// BudgetSourceOrgDefault: an organization's per-run ceiling bound it.
	BudgetSourceOrgDefault = "org_default"
	// BudgetSourceEntitlement: remaining plan entitlement bound it (reserved
	// for the WS6.D resolver; accepted so that minter needs no broker change).
	BudgetSourceEntitlement = "entitlement"
)

// BudgetSources is the closed set of Budget.Source values.
var BudgetSources = map[string]struct{}{
	BudgetSourceRequest:     {},
	BudgetSourceOrgDefault:  {},
	BudgetSourceEntitlement: {},
}

// Budget is a grant's budget claim: an upper bound on what a run under the
// grant may spend. It can only tighten what a consumer would otherwise allow,
// never raise it. Zero in a dimension means no bound on that dimension; the
// zero Budget is no claim at all. It is three numbers and a label, never
// content, so a Zero Data Retention grant may carry one (and, like the rest of
// that grant, it lives only in memory).
type Budget struct {
	// MicroUSD bounds estimated spend, in millionths of a dollar.
	MicroUSD int64
	// MaxSteps bounds completed steps.
	MaxSteps int32
	// MaxRuntimeS bounds active run time, in seconds.
	MaxRuntimeS int32
	// Source is one of BudgetSources; set exactly when a dimension is.
	Source string
	// MintedAtUnix is stamped by the store when the claim is stored, never
	// taken from a request.
	MintedAtUnix int64
}

// IsSet reports whether any dimension is bounded.
func (b Budget) IsSet() bool {
	return b.MicroUSD > 0 || b.MaxSteps > 0 || b.MaxRuntimeS > 0
}

// NormalizeBudget validates a claim as it arrives from a caller. MintedAtUnix
// must be zero: the store owns it.
func NormalizeBudget(b Budget) (Budget, error) {
	if b.MicroUSD < 0 || b.MaxSteps < 0 || b.MaxRuntimeS < 0 || b.MintedAtUnix != 0 {
		return Budget{}, ErrInvalidBudget
	}
	source := strings.TrimSpace(b.Source)
	if !b.IsSet() {
		if source != "" {
			return Budget{}, ErrInvalidBudget
		}
		return Budget{}, nil
	}
	if _, ok := BudgetSources[source]; !ok {
		return Budget{}, ErrInvalidBudget
	}
	b.Source = source
	return b, nil
}

// tightenBudget computes the claim of a run grant derived from a parent that
// holds parent. Each dimension the parent bounds stays bounded: an omitted
// dimension inherits the parent's value, a smaller one is kept, a larger one
// is ErrDeriveWidensBudget. A dimension the parent leaves open may be set
// freely. When the request sets any dimension the claim is labelled with the
// request's source and stamped now; a request that sets none inherits the
// parent's label and stamp unchanged.
func tightenBudget(parent, requested Budget, nowUnix int64) (Budget, error) {
	out := Budget{}
	var err error
	if out.MicroUSD, err = tightenInt(int64(parent.MicroUSD), int64(requested.MicroUSD)); err != nil {
		return Budget{}, err
	}
	steps, err := tightenInt(int64(parent.MaxSteps), int64(requested.MaxSteps))
	if err != nil {
		return Budget{}, err
	}
	runtime, err := tightenInt(int64(parent.MaxRuntimeS), int64(requested.MaxRuntimeS))
	if err != nil {
		return Budget{}, err
	}
	out.MaxSteps, out.MaxRuntimeS = int32(steps), int32(runtime)
	switch {
	case requested.IsSet():
		out.Source = requested.Source
		out.MintedAtUnix = nowUnix
	case parent.IsSet():
		out.Source = parent.Source
		out.MintedAtUnix = parent.MintedAtUnix
	}
	return out, nil
}

func tightenInt(parent, requested int64) (int64, error) {
	switch {
	case parent <= 0:
		return requested, nil
	case requested == 0:
		return parent, nil
	case requested > parent:
		return 0, ErrDeriveWidensBudget
	default:
		return requested, nil
	}
}

// sameBudgetClaim compares what a replay asks for with what was issued: the
// three bounds and the label (the stamp is the broker's own).
func sameBudgetClaim(a, b Budget) bool {
	return a.MicroUSD == b.MicroUSD && a.MaxSteps == b.MaxSteps &&
		a.MaxRuntimeS == b.MaxRuntimeS && a.Source == b.Source
}

// sameDeriveBudget decides whether a replay of a derive agrees with the grant
// it names about the budget claim. A replay that sets a claim of its own (any
// bound or a label) must ask for exactly what was issued: tightenBudget of the
// parent and that request has to equal the issued claim, so a different claim,
// tighter or looser, is a conflict. A replay that sets none is a lookup: it
// asks for no claim, so there is nothing to disagree with, and it gets the
// issued grant back with the claim that grant carries. That is what lets a
// caller whose claim is recomputed on each attempt (from a ceiling that moved,
// or one it could not read) retry a derive and find its grant. It cannot widen
// or refresh anything: the claim returned is the stored one, fixed at the
// first derive, and the stamp is not touched.
func sameDeriveBudget(requested, issued, tightened Budget) bool {
	return !requested.IsSet() || sameBudgetClaim(issued, tightened)
}

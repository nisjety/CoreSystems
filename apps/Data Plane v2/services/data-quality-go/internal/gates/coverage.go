package gates

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/policy"
)

// The statuses a gate that can be unable to judge ends in (model.GateResult.Status).
const (
	StatusPassed        = "passed"
	StatusFailed        = "failed"
	StatusSkippedPolicy = "skipped_policy"
	StatusUnavailable   = "unavailable"
)

// Gate names.
const (
	GateContextualCoverage = "contextual_coverage_ok"
	GateSparseIndexInSync  = "sparse_index_in_sync"
)

// DefaultCoverageThresholdPercent is the share of eligible units that must have a
// verified context for contextual_coverage_ok (the design's 99 percent). The
// service reads it from CONTEXT_COVERAGE_THRESHOLD_PERCENT.
const DefaultCoverageThresholdPercent = 99.0

// ErrCoverageUnavailable is returned when v_context_coverage does not exist: the
// context-provenance migration has not been applied to this database.
var ErrCoverageUnavailable = errors.New("gates: v_context_coverage does not exist (apply migration 20261003130000_context_provenance)")

// CoverageCounts is one row of v_context_coverage: every count is over ELIGIBLE
// units (embedding done, document not deleted, classification internal, public or
// sensitive: the population the keyword index also uses). The statuses partition
// Eligible.
type CoverageCounts struct {
	Eligible                 int64 `json:"eligible"`
	Ready                    int64 `json:"ready"`
	Legacy                   int64 `json:"legacy"`
	Pending                  int64 `json:"pending"`
	SkippedZDR               int64 `json:"skipped_zdr"`
	SkippedPolicy            int64 `json:"skipped_policy"`
	SkippedBudget            int64 `json:"skipped_budget"`
	FailedTransient          int64 `json:"failed_transient"`
	FailedPermanent          int64 `json:"failed_permanent"`
	StatusUnknown            int64 `json:"status_unknown"`
	VectorIncoherent         int64 `json:"vector_incoherent"`
	ReadyUnverified          int64 `json:"ready_unverified"`
	ExcludedByClassification int64 `json:"excluded_by_classification"`
}

// PolicySetting is what the gates need of the organization's processing policy.
type PolicySetting struct {
	// ContextualRetrieval is inherit, on or off.
	ContextualRetrieval string `json:"contextual_retrieval"`
	// Source is "policy" when the organization has a row and "default" when not.
	Source string `json:"source"`
}

func settingOf(record policy.Record) PolicySetting {
	source := "default"
	if record.Exists {
		source = "policy"
	}
	return PolicySetting{ContextualRetrieval: record.ContextualRetrieval, Source: source}
}

// CoverageEvaluation is how the counts judge against the threshold.
type CoverageEvaluation struct {
	// Status is passed, failed or skipped_policy.
	Status string `json:"status"`
	// ReadyTrusted is the numerator: units that are ready AND whose vector is
	// known to have been built from the stored context (ready minus
	// ready_unverified minus vector_incoherent).
	ReadyTrusted int64 `json:"ready_trusted"`
	// Denominator is the units that are expected to have a context: eligible
	// minus those explicitly skipped for zero retention.
	Denominator int64 `json:"denominator"`
	// CoveragePercent is nil when nothing is expected.
	CoveragePercent  *float64 `json:"coverage_percent"`
	ThresholdPercent float64  `json:"threshold_percent"`
	Message          string   `json:"message"`
}

// EvaluateContextCoverage is the rule of the contextual_coverage_ok gate and of
// the coverage endpoint, in one place so they cannot disagree.
//
//   - An organization whose policy is OFF passes, loudly: contextual retrieval was
//     switched off on purpose (an audited operator decision) and its units are
//     marked skipped_policy; coverage is not expected of it.
//   - Units skipped for zero retention (skipped_zdr) leave the denominator: they
//     are withheld on purpose and carry their own marker.
//   - Everything else that is eligible counts against coverage until it is ready:
//     `legacy` (a context of unknown provenance, awaiting the verification pass),
//     `pending`, a stale `skipped_policy` (the policy is not off any more, so the
//     unit is waiting to be generated), `skipped_budget`, the failures, a unit
//     with no status at all, a `ready` unit whose vector is not proven to match
//     its context (ready_unverified) and one whose vector provably does not
//     (vector_incoherent).
func EvaluateContextCoverage(c CoverageCounts, setting PolicySetting, thresholdPercent float64) CoverageEvaluation {
	evaluation := CoverageEvaluation{ThresholdPercent: thresholdPercent}
	if setting.ContextualRetrieval == policy.Off {
		evaluation.Status = StatusSkippedPolicy
		evaluation.Message = fmt.Sprintf(
			"contextual retrieval is OFF for this organization by operator policy; %d of %d eligible units are skipped_policy and coverage is not expected",
			c.SkippedPolicy, c.Eligible)
		return evaluation
	}

	evaluation.ReadyTrusted = max(c.Ready-c.ReadyUnverified-c.VectorIncoherent, 0)
	evaluation.Denominator = max(c.Eligible-c.SkippedZDR, 0)
	if evaluation.Denominator == 0 {
		evaluation.Status = StatusPassed
		evaluation.Message = fmt.Sprintf(
			"no unit is expected to have a context (%d eligible, %d skipped for zero retention)", c.Eligible, c.SkippedZDR)
		return evaluation
	}
	percent := 100 * float64(evaluation.ReadyTrusted) / float64(evaluation.Denominator)
	evaluation.CoveragePercent = &percent
	// Compared as ready*100 >= expected*threshold so that a threshold of 99 on
	// 198 of 200 is exactly on the line, not a rounding error below it.
	if float64(evaluation.ReadyTrusted)*100 >= float64(evaluation.Denominator)*thresholdPercent {
		evaluation.Status = StatusPassed
	} else {
		evaluation.Status = StatusFailed
	}
	evaluation.Message = fmt.Sprintf("%d of %d expected units have a verified context (%.2f%%, threshold %.2f%%)",
		evaluation.ReadyTrusted, evaluation.Denominator, percent, thresholdPercent)
	if rest := notReady(c); rest != "" {
		evaluation.Message += "; not ready: " + rest
	}
	return evaluation
}

// notReady lists, in a stable order, what keeps units from counting as ready.
func notReady(c CoverageCounts) string {
	parts := make([]string, 0, 9)
	add := func(count int64, what string) {
		if count > 0 {
			parts = append(parts, fmt.Sprintf("%d %s", count, what))
		}
	}
	add(c.Legacy, "legacy (provenance unknown, awaiting the verification pass)")
	add(c.Pending, "pending")
	add(c.StatusUnknown, "without a status")
	add(c.ReadyUnverified, "ready but unverified (no vector hash)")
	add(c.VectorIncoherent, "vector incoherent (the vector was not built from the stored context)")
	add(c.SkippedPolicy, "skipped_policy while the policy is not off (waiting to be generated)")
	add(c.SkippedBudget, "skipped_budget")
	add(c.FailedTransient, "failed_transient")
	add(c.FailedPermanent, "failed_permanent")
	return strings.Join(parts, ", ")
}

// GateOf turns an evaluation into the gate result the report carries.
func (e CoverageEvaluation) GateOf() model.GateResult {
	value := "n/a"
	if e.CoveragePercent != nil {
		value = fmt.Sprintf("%.2f", *e.CoveragePercent)
	}
	return model.GateResult{
		Gate:    GateContextualCoverage,
		Passed:  e.Status == StatusPassed || e.Status == StatusSkippedPolicy,
		Status:  e.Status,
		Message: e.Message,
		Value:   value,
	}
}

// unavailableCoverage is the gate when the evidence cannot be read.
func unavailableCoverage(reason string) model.GateResult {
	return model.GateResult{
		Gate:    GateContextualCoverage,
		Passed:  false,
		Status:  StatusUnavailable,
		Message: "contextual coverage cannot be read: " + reason + " (never reported as passing)",
	}
}

const coverageSQL = `
	SELECT eligible, ready, legacy, pending, skipped_zdr, skipped_policy, skipped_budget,
	       failed_transient, failed_permanent, status_unknown, vector_incoherent,
	       ready_unverified, excluded_by_classification
	  FROM v_context_coverage
	 WHERE org_id = $1`

// queryer is the read surface of a scoped transaction or a savepoint in one.
type queryer interface {
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

func isUndefinedRelation(err error) bool {
	var pgErr *pgconn.PgError
	return errors.As(err, &pgErr) && pgErr.Code == "42P01"
}

// readCoverage reads the organization's row of v_context_coverage inside a scope
// (the view is security_invoker: it honours the caller's row-level security). An
// organization with no indexed unit has no row, which is zero counts.
func readCoverage(ctx context.Context, q queryer, orgID string) (CoverageCounts, error) {
	var c CoverageCounts
	err := q.QueryRow(ctx, coverageSQL, orgID).Scan(
		&c.Eligible, &c.Ready, &c.Legacy, &c.Pending, &c.SkippedZDR, &c.SkippedPolicy, &c.SkippedBudget,
		&c.FailedTransient, &c.FailedPermanent, &c.StatusUnknown, &c.VectorIncoherent,
		&c.ReadyUnverified, &c.ExcludedByClassification,
	)
	switch {
	case errors.Is(err, pgx.ErrNoRows):
		return CoverageCounts{}, nil
	case isUndefinedRelation(err):
		return CoverageCounts{}, fmt.Errorf("%w: %v", ErrCoverageUnavailable, err)
	case err != nil:
		return CoverageCounts{}, err
	}
	return c, nil
}

// CoverageReport is what GET /v1/quality/context-coverage returns.
type CoverageReport struct {
	OrgID      string             `json:"org_id"`
	Counts     CoverageCounts     `json:"counts"`
	Policy     PolicySetting      `json:"policy"`
	Evaluation CoverageEvaluation `json:"evaluation"`
}

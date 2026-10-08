package gates

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/policy"
)

var inheritSetting = PolicySetting{ContextualRetrieval: policy.Inherit, Source: "default"}
var offSetting = PolicySetting{ContextualRetrieval: policy.Off, Source: "policy"}

// counts builds a fixture of eligible units: every unit is in exactly one status, so
// the statuses partition `eligible` (the view's own invariant).
func counts(ready, legacy, pending, skippedZDR, skippedPolicy int64) CoverageCounts {
	return CoverageCounts{
		Eligible:      ready + legacy + pending + skippedZDR + skippedPolicy,
		Ready:         ready,
		Legacy:        legacy,
		Pending:       pending,
		SkippedZDR:    skippedZDR,
		SkippedPolicy: skippedPolicy,
	}
}

func TestACoverageFixtureAt98PercentFailsTheGate(t *testing.T) {
	// 196 of 200 ready, 4 pending: 98%.
	evaluation := EvaluateContextCoverage(counts(196, 0, 4, 0, 0), inheritSetting, 99)
	if evaluation.Status != StatusFailed {
		t.Fatalf("status = %s (%s), want failed at 98%% against 99%%", evaluation.Status, evaluation.Message)
	}
	gate := evaluation.GateOf()
	if gate.Passed || gate.Gate != GateContextualCoverage || gate.Status != StatusFailed {
		t.Fatalf("gate = %+v", gate)
	}
	if evaluation.CoveragePercent == nil || *evaluation.CoveragePercent != 98 {
		t.Fatalf("coverage = %v, want 98", evaluation.CoveragePercent)
	}
	if !strings.Contains(evaluation.Message, "4 pending") {
		t.Fatalf("the message does not say what is not ready: %s", evaluation.Message)
	}
}

func TestTheThresholdIsInclusiveAndTheSameFixtureAt99PassesAndAbovePasses(t *testing.T) {
	// 198 of 200: exactly 99%, on the line, and not a rounding error below it.
	if got := EvaluateContextCoverage(counts(198, 0, 2, 0, 0), inheritSetting, 99); got.Status != StatusPassed {
		t.Fatalf("198/200 at 99%% = %s (%s), want passed", got.Status, got.Message)
	}
	if got := EvaluateContextCoverage(counts(200, 0, 0, 0, 0), inheritSetting, 99); got.Status != StatusPassed {
		t.Fatalf("200/200 = %s, want passed", got.Status)
	}
	// 197 of 200 is 98.5%: below.
	if got := EvaluateContextCoverage(counts(197, 0, 3, 0, 0), inheritSetting, 99); got.Status != StatusFailed {
		t.Fatalf("197/200 = %s, want failed", got.Status)
	}
	// A configurable threshold: the same 98% passes at 95 and fails at 99.9.
	if got := EvaluateContextCoverage(counts(196, 0, 4, 0, 0), inheritSetting, 95); got.Status != StatusPassed {
		t.Fatalf("98%% at a 95%% threshold = %s, want passed", got.Status)
	}
	if got := EvaluateContextCoverage(counts(199, 0, 1, 0, 0), inheritSetting, 99.9); got.Status != StatusFailed {
		t.Fatalf("99.5%% at a 99.9%% threshold = %s, want failed", got.Status)
	}
}

// The live state of 2026-10-06 (207 units, 206 with a context of unknown provenance, 1
// raw): until the legacy verification pass runs, legacy is eligible but NOT ready. And
// after it, the same organization is covered.
func TestLegacyIsEligibleButNotReadyUntilTheVerificationPassRuns(t *testing.T) {
	before := EvaluateContextCoverage(counts(0, 206, 1, 0, 0), inheritSetting, 99)
	if before.Status != StatusFailed || before.ReadyTrusted != 0 || before.Denominator != 207 {
		t.Fatalf("0 ready + 206 legacy + 1 pending = %+v, want failed 0/207", before)
	}
	if !strings.Contains(before.Message, "206 legacy") || !strings.Contains(before.Message, "1 pending") {
		t.Fatalf("the message hides what is waiting: %s", before.Message)
	}
	after := EvaluateContextCoverage(counts(207, 0, 0, 0, 0), inheritSetting, 99)
	if after.Status != StatusPassed || after.ReadyTrusted != 207 {
		t.Fatalf("207/207 ready = %+v, want passed", after)
	}
}

// A skipped_policy organization passes, loudly: contextual retrieval was switched off on
// purpose, and the units are marked rather than silently raw.
func TestAnOrganizationWhosePolicyIsOffPassesAndSaysSo(t *testing.T) {
	evaluation := EvaluateContextCoverage(counts(0, 0, 0, 0, 207), offSetting, 99)
	if evaluation.Status != StatusSkippedPolicy {
		t.Fatalf("status = %s, want skipped_policy", evaluation.Status)
	}
	gate := evaluation.GateOf()
	if !gate.Passed || gate.Status != StatusSkippedPolicy {
		t.Fatalf("gate = %+v, want a pass that says it was skipped by policy", gate)
	}
	if !strings.Contains(gate.Message, "OFF") || !strings.Contains(gate.Message, "207") {
		t.Fatalf("an off policy passes silently: %q", gate.Message)
	}
	// Even a cold organization with nothing ready passes while its policy is off.
	if got := EvaluateContextCoverage(counts(0, 0, 40, 0, 0), offSetting, 99); got.Status != StatusSkippedPolicy {
		t.Fatalf("pending units under an off policy = %s, want skipped_policy", got.Status)
	}
}

// The other direction: skipped_policy units while the policy is NOT off are stale. The
// policy was lifted and the units are waiting to be generated; they must not be excused.
func TestStaleSkippedPolicyUnitsAreNotExcusedOnceThePolicyIsLifted(t *testing.T) {
	evaluation := EvaluateContextCoverage(counts(10, 0, 0, 0, 190), inheritSetting, 99)
	if evaluation.Status != StatusFailed || evaluation.Denominator != 200 || evaluation.ReadyTrusted != 10 {
		t.Fatalf("%+v: stale skipped_policy units were excused", evaluation)
	}
	if !strings.Contains(evaluation.Message, "skipped_policy while the policy is not off") {
		t.Fatalf("the message does not name the stale units: %s", evaluation.Message)
	}
}

func TestZeroRetentionSkipsLeaveTheDenominator(t *testing.T) {
	// 100 ready and 100 withheld for zero retention: every unit that is expected to have a context has one.
	evaluation := EvaluateContextCoverage(counts(100, 0, 0, 100, 0), inheritSetting, 99)
	if evaluation.Status != StatusPassed || evaluation.Denominator != 100 {
		t.Fatalf("%+v: skipped_zdr units were counted against coverage", evaluation)
	}
	// A fully zero-retention organization expects nothing and passes without a division by zero.
	all := EvaluateContextCoverage(counts(0, 0, 0, 50, 0), inheritSetting, 99)
	if all.Status != StatusPassed || all.CoveragePercent != nil || all.Denominator != 0 {
		t.Fatalf("%+v: an organization with nothing expected must pass vacuously", all)
	}
	if gate := all.GateOf(); gate.Value != "n/a" || !gate.Passed {
		t.Fatalf("gate = %+v", gate)
	}
}

func TestAnOrganizationWithNoUnitsPassesVacuously(t *testing.T) {
	if got := EvaluateContextCoverage(CoverageCounts{}, inheritSetting, 99); got.Status != StatusPassed {
		t.Fatalf("no units = %s, want passed (nothing is expected)", got.Status)
	}
}

// A `ready` unit is trusted only when its vector is known to have been built from the
// stored context. Unverified and incoherent units are subtracted from the numerator.
func TestReadyUnitsWhoseVectorIsNotProvenAreNotCounted(t *testing.T) {
	c := counts(200, 0, 0, 0, 0)
	c.ReadyUnverified = 3
	c.VectorIncoherent = 2
	evaluation := EvaluateContextCoverage(c, inheritSetting, 99)
	if evaluation.ReadyTrusted != 195 || evaluation.Status != StatusFailed {
		t.Fatalf("%+v: 195 of 200 (97.5%%) must fail", evaluation)
	}
	for _, want := range []string{"3 ready but unverified", "2 vector incoherent"} {
		if !strings.Contains(evaluation.Message, want) {
			t.Fatalf("message %q lacks %q", evaluation.Message, want)
		}
	}
	// One unverified unit among 1000 stays inside the threshold.
	big := counts(1000, 0, 0, 0, 0)
	big.ReadyUnverified = 1
	if got := EvaluateContextCoverage(big, inheritSetting, 99); got.Status != StatusPassed {
		t.Fatalf("1 unverified of 1000 = %s, want passed", got.Status)
	}
}

func TestEveryNotReadyStatusCountsAgainstCoverage(t *testing.T) {
	c := CoverageCounts{
		Eligible: 100, Ready: 90,
		SkippedBudget: 2, FailedTransient: 3, FailedPermanent: 2, StatusUnknown: 3,
	}
	evaluation := EvaluateContextCoverage(c, inheritSetting, 99)
	if evaluation.Status != StatusFailed || evaluation.ReadyTrusted != 90 {
		t.Fatalf("%+v", evaluation)
	}
	for _, want := range []string{"2 skipped_budget", "3 failed_transient", "2 failed_permanent", "3 without a status"} {
		if !strings.Contains(evaluation.Message, want) {
			t.Fatalf("message %q lacks %q", evaluation.Message, want)
		}
	}
}

// Counts that do not partition (a view race) must never produce a negative numerator.
func TestTheNumeratorNeverGoesNegative(t *testing.T) {
	c := CoverageCounts{Eligible: 10, Ready: 1, ReadyUnverified: 5, VectorIncoherent: 5}
	if got := EvaluateContextCoverage(c, inheritSetting, 99); got.ReadyTrusted != 0 || got.Status != StatusFailed {
		t.Fatalf("%+v", got)
	}
}

func TestAnUnavailableCoverageGateIsNeverAPass(t *testing.T) {
	gate := unavailableCoverage("the view is missing")
	if gate.Passed || gate.Status != StatusUnavailable || gate.Gate != GateContextualCoverage {
		t.Fatalf("gate = %+v, want unavailable and not passed", gate)
	}
	if !strings.Contains(gate.Message, "never reported as passing") {
		t.Fatalf("message = %q", gate.Message)
	}
}

// rowStub is the one row a queryer returns: an error from Scan, as the database would raise it.
type rowStub struct{ err error }

func (r rowStub) Scan(...any) error { return r.err }

type queryerStub struct{ err error }

func (q queryerStub) QueryRow(context.Context, string, ...any) pgx.Row { return rowStub{err: q.err} }

// A database without the migration is "unavailable", and an organization with nothing indexed is zero
// counts: neither is an error to hide or to pass through as a 500.
func TestReadCoverageMapsAMissingViewAndAnEmptyOrganization(t *testing.T) {
	_, err := readCoverage(context.Background(), queryerStub{err: &pgconn.PgError{Code: "42P01", Message: "relation \"v_context_coverage\" does not exist"}}, "org-a")
	if !errors.Is(err, ErrCoverageUnavailable) {
		t.Fatalf("err = %v, want ErrCoverageUnavailable", err)
	}
	counts, err := readCoverage(context.Background(), queryerStub{err: pgx.ErrNoRows}, "org-a")
	if err != nil || counts != (CoverageCounts{}) {
		t.Fatalf("an organization with no indexed unit = %+v, %v, want zero counts", counts, err)
	}
	boom := errors.New("connection reset")
	if _, err := readCoverage(context.Background(), queryerStub{err: boom}, "org-a"); !errors.Is(err, boom) || errors.Is(err, ErrCoverageUnavailable) {
		t.Fatalf("another failure = %v, want it returned as it is", err)
	}
}

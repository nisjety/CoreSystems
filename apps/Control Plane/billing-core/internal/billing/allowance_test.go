package billing

import (
	"testing"
	"time"
)

func allowanceFixture() AllowancePolicy {
	return AllowancePolicy{Metric: "ai_tokens", Name: "AI tokens", Unit: "tokens", Version: "v1", StartsAt: time.Date(2026, 1, 31, 12, 0, 0, 0, time.UTC), IntervalMonths: 1, IncludedUnits: 100, RolloverCap: 100, RolloverCycles: 1}
}
func allowanceUsage(id string, n int64, at time.Time) AllowanceEntry {
	return AllowanceEntry{ID: id, Kind: "usage", Units: n, OccurredAt: at}
}

func TestAllowanceScheduledChangePreservesPriorPeriods(t *testing.T) {
	p := allowanceFixture()
	next := allowanceBoundary(p, 1)
	p.Revisions = []AllowanceRevision{{Version: "v2", EffectiveAt: next, IncludedUnits: 200, RolloverCap: 50, RolloverCycles: 1, Reason: "Upgrade at renewal"}}
	events := []AllowanceEntry{allowanceUsage("before", 40, p.StartsAt.Add(time.Hour)), allowanceUsage("after", 10, next)}
	before, err := CalculateAllowance(p, events, next.Add(-time.Second))
	if err != nil || before.NextChange == nil || before.Current.PolicyVersion != "v1" || before.Current.Used != 40 {
		t.Fatalf("before change: %+v %v", before, err)
	}
	after, err := CalculateAllowance(p, events, next)
	if err != nil || after.NextChange != nil || after.Current.PolicyVersion != "v2" || after.Current.Included != 200 || after.Current.Carried != 50 || after.Current.Remaining != 240 || after.History[0].Included != 100 || after.History[0].Used != 40 {
		t.Fatalf("after change: %+v %v", after, err)
	}
	// Late usage is still attributed to v1, correcting the amount carried to v2.
	events = append(events, allowanceUsage("late", 50, p.StartsAt.Add(time.Minute)))
	corrected, err := CalculateAllowance(p, events, next)
	if err != nil || corrected.Current.Carried != 10 || corrected.Current.Remaining != 200 {
		t.Fatalf("late correction: %+v %v", corrected, err)
	}
	p.Revisions[0].EffectiveAt = next.Add(time.Hour)
	if _, err := CalculateAllowance(p, nil, next); err == nil {
		t.Fatal("mid-cycle change accepted")
	}
}

func TestAllowanceEndsAtExactContractBoundary(t *testing.T) {
	p := allowanceFixture()
	end := p.StartsAt.Add(24 * time.Hour)
	p.EndsAt = &end
	r, err := CalculateAllowance(p, nil, end.Add(time.Hour))
	if err != nil || r.Active || !r.Current.To.Equal(end) || len(r.History) != 1 {
		t.Fatalf("incorrect final period: %+v %v", r, err)
	}
}
func TestAllowanceRolloverExpiryAndAnchor(t *testing.T) {
	p := allowanceFixture()
	feb := allowanceBoundary(p, 1)
	mar := allowanceBoundary(p, 2)
	if feb.Day() != 28 || mar.Day() != 31 {
		t.Fatal("month-end anchor drift")
	}
	events := []AllowanceEntry{allowanceUsage("a", 40, p.StartsAt.Add(time.Hour)), allowanceUsage("b", 20, feb.Add(time.Hour))}
	r, err := CalculateAllowance(p, events, mar.Add(time.Hour))
	if err != nil {
		t.Fatal(err)
	}
	if r.Current.Carried != 100 || r.Current.Remaining != 200 || r.History[1].Used != 20 || r.History[1].Expired != 40 {
		t.Fatalf("unexpected rollover: %+v", r)
	}
}
func TestAllowanceLateUsageReconcilesCarry(t *testing.T) {
	p := allowanceFixture()
	now := allowanceBoundary(p, 1).Add(time.Hour)
	r, err := CalculateAllowance(p, []AllowanceEntry{allowanceUsage("late", 90, p.StartsAt.Add(time.Minute))}, now)
	if err != nil || r.Current.Carried != 10 || r.Current.Remaining != 110 {
		t.Fatalf("late usage lost: %+v %v", r, err)
	}
}
func TestAllowanceGrantExpiryAndOverage(t *testing.T) {
	p := allowanceFixture()
	p.IncludedUnits = 0
	p.RolloverCap = 0
	expiry := p.StartsAt.Add(2 * time.Hour)
	entries := []AllowanceEntry{{ID: "purchase", Kind: "grant", Units: 50, OccurredAt: p.StartsAt, ExpiresAt: &expiry}, allowanceUsage("before", 20, p.StartsAt.Add(time.Hour)), allowanceUsage("at-expiry", 10, expiry)}
	r, err := CalculateAllowance(p, entries, expiry)
	if err != nil {
		t.Fatal(err)
	}
	if r.Current.Expired != 30 || r.Current.Overage != 10 || r.Current.Remaining != 0 {
		t.Fatalf("expiry boundary wrong: %+v", r)
	}
}
func TestAllowanceResetNeverDropsHistory(t *testing.T) {
	p := allowanceFixture()
	p.RolloverCycles = 0
	r, err := CalculateAllowance(p, []AllowanceEntry{allowanceUsage("old", 130, p.StartsAt), allowanceUsage("new", 5, allowanceBoundary(p, 1))}, allowanceBoundary(p, 1))
	if err != nil || len(r.History) != 1 || r.History[0].Used != 130 || r.History[0].Overage != 30 || r.Current.Used != 5 || r.Current.Remaining != 95 {
		t.Fatalf("period reset wrong: %+v %v", r, err)
	}
}
func TestAllowanceUnlimitedRolloverAndRetryDeterminism(t *testing.T) {
	p := allowanceFixture()
	p.RolloverCycles = -1
	p.RolloverCap = 1000
	for i := 0; i < 2; i++ {
		r, err := CalculateAllowance(p, nil, allowanceBoundary(p, 4))
		if err != nil || r.Current.Remaining != 500 {
			t.Fatalf("replay changed grants: %+v %v", r, err)
		}
	}
}
func TestAllowanceRejectsDuplicateAndInvalidEntries(t *testing.T) {
	p := allowanceFixture()
	e := allowanceUsage("same", 1, p.StartsAt)
	if _, err := CalculateAllowance(p, []AllowanceEntry{e, e}, p.StartsAt); err != ErrAllowanceConflict {
		t.Fatal(err)
	}
	e.Units = -1
	if _, err := CalculateAllowance(p, []AllowanceEntry{e}, p.StartsAt); err == nil {
		t.Fatal("negative units accepted")
	}
}
func TestAllowanceEndedPolicyStopsRenewal(t *testing.T) {
	p := allowanceFixture()
	end := allowanceBoundary(p, 1)
	p.EndsAt = &end
	r, err := CalculateAllowance(p, nil, allowanceBoundary(p, 3))
	if err != nil || r.Active || len(r.History) != 1 {
		t.Fatalf("ended allowance renewed: %+v %v", r, err)
	}
}

func TestAllowancePausedCyclesDoNotGrantAndRecoveryDoesNotBackdate(t *testing.T) {
	p := allowanceFixture()
	p.RolloverCycles = 0
	resume := allowanceBoundary(p, 2).Add(time.Hour)
	p.Pauses = []AllowancePause{{From: allowanceBoundary(p, 1).Add(-time.Hour), To: &resume}}
	r, err := CalculateAllowance(p, nil, resume.Add(time.Hour))
	if err != nil || !r.Active || r.Current.Included != 0 || r.Current.Remaining != 0 {
		t.Fatalf("unpaid cycles granted: %+v %v", r, err)
	}
	r, err = CalculateAllowance(p, nil, allowanceBoundary(p, 3))
	if err != nil || r.Current.Included != 100 {
		t.Fatal("next cycle did not resume", r, err)
	}
}

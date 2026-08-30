package fleet

import "testing"

func TestFleetTask_AddMemberIsIdempotent(t *testing.T) {
	reg := NewRegistry()
	task := &Task{FleetID: "fleet_1", OrgID: "org_a", MaxParallelRuns: 3}
	if _, err := reg.Create(task); err != nil {
		t.Fatalf("create: %v", err)
	}
	if err := reg.AddMember("fleet_1", "run_1"); err != nil {
		t.Fatalf("add member: %v", err)
	}
	if err := reg.AddMember("fleet_1", "run_1"); err != nil {
		t.Fatalf("add member idempotent: %v", err)
	}
	got, _ := reg.Get("fleet_1")
	if len(got.MemberRunIDs) != 1 {
		t.Fatalf("expected 1 member, got %d", len(got.MemberRunIDs))
	}
}

func TestBudgetTrackerAggregatesAcrossMembersAndTrips(t *testing.T) {
	budget := 1.0
	tracker := NewBudgetTracker(&budget)
	tracker.Record("run_a", 0.30)
	tracker.Record("run_b", 0.40)
	tracker.Record("run_c", 0.35)
	if _, _, over := tracker.OverBudget(); !over {
		t.Fatal("expected over budget (1.05 >= 1.0)")
	}
	spent, limit, _ := tracker.OverBudget()
	if spent < 1.04 || spent > 1.06 {
		t.Fatalf("spent %f", spent)
	}
	if limit != 1.0 {
		t.Fatalf("limit %f", limit)
	}
}

func TestBudgetTrackerBackPressureOnOverBudget(t *testing.T) {
	budget := 0.009
	tracker := NewBudgetTracker(&budget)
	tracker.RecordMicro("run_a", 5000)
	if _, _, over := tracker.OverBudget(); over {
		t.Fatal("should not be over after 0.005")
	}
	tracker.RecordMicro("run_b", 5000)
	if _, _, over := tracker.OverBudget(); !over {
		t.Fatal("should be over after 0.010")
	}
}

func TestRegistryCreateAndGet(t *testing.T) {
	reg := NewRegistry()
	budget := 5.0
	task := &Task{FleetID: "fleet_2", OrgID: "org_a", MaxParallelRuns: 2, BudgetUSD: &budget}
	if _, err := reg.Create(task); err != nil {
		t.Fatalf("create: %v", err)
	}
	got, ok := reg.Get("fleet_2")
	if !ok || got.OrgID != "org_a" || got.MaxParallelRuns != 2 {
		t.Fatalf("get: %v %v", ok, got)
	}
	if _, ok := reg.Tracker("fleet_2"); !ok {
		t.Fatal("tracker missing")
	}
}

func TestRegistryRejectsDuplicateFleetID(t *testing.T) {
	reg := NewRegistry()
	task := &Task{FleetID: "fleet_dup", OrgID: "org_a", MaxParallelRuns: 2}
	if _, err := reg.Create(task); err != nil {
		t.Fatalf("first create: %v", err)
	}
	dup := &Task{FleetID: "fleet_dup", OrgID: "org_a", MaxParallelRuns: 2}
	if _, err := reg.Create(dup); err == nil {
		t.Fatal("expected duplicate error")
	}
}

func TestSubjectHelpers(t *testing.T) {
	if got := Subject("fleet_abc", "agent.started"); got != "quarry.fleet.fleet_abc.agent.started" {
		t.Fatalf("subject %s", got)
	}
	if got := Wildcard("fleet_abc"); got != "quarry.fleet.fleet_abc.>" {
		t.Fatalf("wildcard %s", got)
	}
}

func TestRegistryFinishSetsStatusAndTimestamp(t *testing.T) {
	reg := NewRegistry()
	task := &Task{FleetID: "fleet_3", OrgID: "org_a", MaxParallelRuns: 1}
	if _, err := reg.Create(task); err != nil {
		t.Fatalf("create: %v", err)
	}
	if err := reg.Finish("fleet_3", StatusCompleted); err != nil {
		t.Fatalf("finish: %v", err)
	}
	got, _ := reg.Get("fleet_3")
	if got.Status != StatusCompleted || got.FinishedAt == nil {
		t.Fatalf("finish not applied: %+v", got)
	}
}

func TestRegistryClampsMaxParallel(t *testing.T) {
	reg := NewRegistry()
	task := &Task{FleetID: "fleet_clamp", OrgID: "org_a", MaxParallelRuns: 0}
	if _, err := reg.Create(task); err != nil {
		t.Fatalf("create: %v", err)
	}
	got, _ := reg.Get("fleet_clamp")
	if got.MaxParallelRuns != 1 {
		t.Fatalf("expected clamp to 1, got %d", got.MaxParallelRuns)
	}
}

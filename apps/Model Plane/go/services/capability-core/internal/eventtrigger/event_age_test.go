package eventtrigger

import (
	"context"
	"testing"
	"time"
)

// The webhook event's own age is bounded by the same window as the authorisation (fireWindow). A fire of an event older than
// the window is recorded refused at intake, is never authorised by the sweeper and is never started. These tests pin the rule
// where it is decided; event_age_integration_test.go pins the Postgres side.

// The window's edge is inclusive: an event created exactly fireWindow before the clock is still inside it, and one a nanosecond
// older is not. An event with no creation time is never inside the window.
func TestEventExpiredIsInclusiveAtTheWindowEdge(t *testing.T) {
	now := testNow
	if eventExpired(now.Add(-fireWindow), now) {
		t.Fatal("an event created exactly one window ago was refused; the window includes its edge")
	}
	if !eventExpired(now.Add(-fireWindow-time.Nanosecond), now) {
		t.Fatal("an event one nanosecond past the window was not refused")
	}
	if eventExpired(now, now) {
		t.Fatal("a fresh event was refused")
	}
	if !eventExpired(time.Time{}, now) {
		t.Fatal("an event with no creation time was not refused; its age cannot be shown to be inside the window")
	}
}

// At intake a fresh event's fire is pending for the sweeper. An event already older than the window is recorded refused with
// fire_expired and settled at the intake's clock, so it never reaches the sweeper or the starter.
func TestIntakeKeepsAFreshEventPendingAndRefusesAnOldOneFireExpired(t *testing.T) {
	now := testNow

	status, outcome, settled := intakeFor(now.Add(-time.Minute), now)
	if status != StatusPending || outcome != "" || settled != nil {
		t.Fatalf("a fresh event intakes as %s with outcome %q settled %v; want pending and unsettled", status, outcome, settled)
	}

	status, _, _ = intakeFor(now.Add(-fireWindow), now)
	if status != StatusPending {
		t.Fatalf("an event on the window's edge intakes as %s; want pending", status)
	}

	status, outcome, settled = intakeFor(now.Add(-2*fireWindow), now)
	if status != StatusRefused || outcome != OutcomeFireExpired || settled == nil || !settled.Equal(now) {
		t.Fatalf("an old event intakes as %s with outcome %q settled %v; want refused fire_expired, settled now", status, outcome, settled)
	}

	status, outcome, _ = intakeFor(time.Time{}, now)
	if status != StatusRefused || outcome != OutcomeFireExpired {
		t.Fatalf("an event with no creation time intakes as %s with outcome %q; want refused fire_expired", status, outcome)
	}
}

// A pending fire whose event has aged past the window before the sweep reaches it is refused fire_expired and is never asked of
// Control. This is the backlog case: the fire was pending while its event was fresh, and the sweep finds it too late.
func TestRunOnceRefusesAFireWhoseEventIsOlderThanTheWindowWithoutAskingControl(t *testing.T) {
	row := pendingRow("trgfire_aged", "trg_1", "wh-aged")
	row.fire.EventAt = testNow.Add(-fireWindow - time.Minute)
	mem := &memLedger{rows: []memRow{row}}
	authority := &fakeAuthorizer{}
	authorized, err := newTestSweeper(mem, authority, testLimits).RunOnce(context.Background())
	if err != nil || authorized != 0 {
		t.Fatalf("RunOnce = %d, %v; want 0, nil", authorized, err)
	}
	if got := mem.rows[0]; got.status != StatusRefused || got.outcome != OutcomeFireExpired || got.attempts != 0 {
		t.Fatalf("row = %+v; want refused fire_expired with no attempt", got)
	}
	if len(authority.intents) != 0 {
		t.Fatal("Control was asked about an event older than the window")
	}
}

// A fire whose event is exactly on the window's edge is still authorised: the check refuses only what is past the window.
func TestRunOnceAuthorisesAFireWhoseEventIsOnTheWindowsEdge(t *testing.T) {
	row := pendingRow("trgfire_edge", "trg_1", "wh-edge")
	row.fire.EventAt = testNow.Add(-fireWindow)
	mem := &memLedger{rows: []memRow{row}}
	authorized, err := newTestSweeper(mem, &fakeAuthorizer{}, testLimits).RunOnce(context.Background())
	if err != nil || authorized != 1 {
		t.Fatalf("RunOnce = %d, %v; want 1, nil", authorized, err)
	}
}

// The event check comes after the trigger check and before the budgets: an event past the window is refused fire_expired even
// when the budgets are spent, and a paused trigger keeps trigger_not_active ahead of the event's age.
func TestRefusalChecksTheEventAgeAfterTheTriggerAndBeforeTheBudgets(t *testing.T) {
	fire := pendingRow("trgfire_1", "trg_1", "wh-1").fire
	fire.EventAt = testNow.Add(-2 * fireWindow)
	if got := refusalFor(fire, Counts{Trigger: 99, Org: 99}, testLimits, testNow); got != OutcomeFireExpired {
		t.Fatalf("refusalFor = %q; want fire_expired ahead of the budgets", got)
	}
	fire.TriggerStatus = "paused"
	if got := refusalFor(fire, Counts{}, testLimits, testNow); got != OutcomeTriggerNotActive {
		t.Fatalf("refusalFor = %q; want trigger_not_active ahead of the event's age", got)
	}
}

// An event with no creation time is refused, never authorised: its age cannot be shown to be inside the window.
func TestRunOnceRefusesAFireWhoseEventHasNoCreationTime(t *testing.T) {
	row := pendingRow("trgfire_untimed", "trg_1", "wh-untimed")
	row.fire.EventAt = time.Time{}
	mem := &memLedger{rows: []memRow{row}}
	authority := &fakeAuthorizer{}
	if _, err := newTestSweeper(mem, authority, testLimits).RunOnce(context.Background()); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if got := mem.rows[0]; got.status != StatusRefused || got.outcome != OutcomeFireExpired {
		t.Fatalf("row = %+v; want refused fire_expired", got)
	}
	if len(authority.intents) != 0 {
		t.Fatal("Control was asked about an event with no creation time")
	}
}

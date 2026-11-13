package eventtrigger

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// A fire of a trigger that is paused or disabled never starts a run. The starter refuses it, and does not leave it
// authorised for the trigger's next activation, where it would start the run of an event that arrived before the pause.
// These tests drive the starter over the in-memory ledger, whose claims apply the trigger condition as the Postgres
// claims do, and pin the claim queries with a capturing transaction. starter_trigger_inactive_integration_test.go pins
// the same behaviour against Postgres.

// schedGateTriggerFire is an authorised fire of a trigger in the given status, whose schedule has the given enabled
// state.
func schedGateTriggerFire(fireID, triggerStatus string, scheduleEnabled bool) *schedGateFire {
	f := schedGateAuthorised(fireID, scheduleEnabled)
	f.triggerStatus = triggerStatus
	return f
}

// A fire of a paused trigger is refused with trigger_not_active, although its schedule is enabled, and writes no task.
func TestStarterRefusesAFireOfAPausedTriggerWithoutStartingIt(t *testing.T) {
	l := &schedGateLedger{fires: []*schedGateFire{schedGateTriggerFire("trgfire_paused", "paused", true)}}
	started, err := newStarter(l).RunOnce(context.Background())
	if err != nil || started != 0 {
		t.Fatalf("RunOnce = %d, %v; want no run started and no error", started, err)
	}
	if f := l.fires[0]; f.status != "refused" || f.outcome != OutcomeTriggerNotActive {
		t.Fatalf("fire is %s with outcome %q; want refused with %q", f.status, f.outcome, OutcomeTriggerNotActive)
	}
	if l.tasks != 0 {
		t.Fatalf("%d tasks written for a paused trigger; want none", l.tasks)
	}
}

// A disabled trigger is final, and its fires are refused the same way.
func TestStarterRefusesAFireOfADisabledTriggerWithoutStartingIt(t *testing.T) {
	l := &schedGateLedger{fires: []*schedGateFire{schedGateTriggerFire("trgfire_disabled", "disabled", true)}}
	started, err := newStarter(l).RunOnce(context.Background())
	if err != nil || started != 0 {
		t.Fatalf("RunOnce = %d, %v; want no run started and no error", started, err)
	}
	if f := l.fires[0]; f.status != "refused" || f.outcome != OutcomeTriggerNotActive {
		t.Fatalf("fire is %s with outcome %q; want refused with %q", f.status, f.outcome, OutcomeTriggerNotActive)
	}
	if l.tasks != 0 {
		t.Fatalf("%d tasks written for a disabled trigger; want none", l.tasks)
	}
}

// The pass that finds a trigger paused refuses its authorised fire. A later re-activation then starts nothing for the
// old event, because the fire is no longer authorised and no claim can take it.
func TestAPausedTriggersFireIsNotStartedWhenTheTriggerIsReactivated(t *testing.T) {
	f := schedGateTriggerFire("trgfire_reactivated", "paused", true)
	l := &schedGateLedger{fires: []*schedGateFire{f}}
	if started, err := newStarter(l).RunOnce(context.Background()); err != nil || started != 0 {
		t.Fatalf("the pass while paused: RunOnce = %d, %v; want no run started and no error", started, err)
	}
	f.triggerStatus = "active"
	if started, err := newStarter(l).RunOnce(context.Background()); err != nil || started != 0 {
		t.Fatalf("the pass after re-activation: RunOnce = %d, %v; want no run started and no error", started, err)
	}
	if f.status != "refused" || f.outcome != OutcomeTriggerNotActive || l.tasks != 0 {
		t.Fatalf("fire is %s with outcome %q and %d tasks after re-activation; want refused with %q and no task",
			f.status, f.outcome, l.tasks, OutcomeTriggerNotActive)
	}
}

// The trigger is checked first, as the sweeper checks it, so a fire of a paused or disabled trigger keeps
// trigger_not_active whatever else is wrong with it.
func TestAFireOfAnInactiveTriggerKeepsTriggerNotActiveWhateverElseIsWrong(t *testing.T) {
	cases := []struct {
		name          string
		triggerStatus string
		mutate        func(*schedGateFire)
	}{
		{
			name:          "a paused trigger with an enabled schedule",
			triggerStatus: "paused",
			mutate:        func(*schedGateFire) {},
		},
		{
			name:          "a disabled trigger with an enabled schedule",
			triggerStatus: "disabled",
			mutate:        func(*schedGateFire) {},
		},
		{
			name:          "a paused trigger whose schedule is switched off",
			triggerStatus: "paused",
			mutate:        func(f *schedGateFire) { f.enabled = false },
		},
		{
			name:          "a paused trigger whose fire key is invalid",
			triggerStatus: "paused",
			mutate:        func(f *schedGateFire) { f.candidate.WebhookEventID = "evt/bad" },
		},
		{
			name:          "a paused trigger whose consent has lapsed",
			triggerStatus: "paused",
			mutate: func(f *schedGateFire) {
				f.candidate.Schedule.Template = json.RawMessage(`{"approval_contract":5}`)
			},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			f := schedGateTriggerFire("trgfire_case", tc.triggerStatus, true)
			tc.mutate(f)
			l := &schedGateLedger{fires: []*schedGateFire{f}}
			if _, err := newStarter(l).RunOnce(context.Background()); err != nil {
				t.Fatalf("RunOnce: %v", err)
			}
			if f.status != "refused" || f.outcome != OutcomeTriggerNotActive || l.tasks != 0 {
				t.Fatalf("fire is %s with outcome %q and %d tasks; want refused with %q and no task",
					f.status, f.outcome, l.tasks, OutcomeTriggerNotActive)
			}
		})
	}
}

// An active trigger's fire still starts in the same pass that refuses a paused trigger's fire, and only the active one
// takes a task.
func TestStarterStartsAnActiveFireAndRefusesAPausedOneInTheSamePass(t *testing.T) {
	l := &schedGateLedger{fires: []*schedGateFire{
		schedGateTriggerFire("trgfire_paused_same", "paused", true),
		schedGateTriggerFire("trgfire_active_same", "active", true),
	}}
	started, err := newStarter(l).RunOnce(context.Background())
	if err != nil || started != 1 {
		t.Fatalf("RunOnce = %d, %v; want one run started and no error", started, err)
	}
	if on := l.fires[1]; on.status != "started" || on.taskID != "task_trgfire_active_same" {
		t.Fatalf("active fire is %s with task %q; want started with its task", on.status, on.taskID)
	}
	if paused := l.fires[0]; paused.status != "refused" || paused.outcome != OutcomeTriggerNotActive {
		t.Fatalf("paused fire is %s with outcome %q; want refused with %q",
			paused.status, paused.outcome, OutcomeTriggerNotActive)
	}
	if l.tasks != 1 {
		t.Fatalf("%d tasks written; want one, for the active trigger's fire", l.tasks)
	}
}

// The start claims keep the active-trigger condition, so neither takes a paused or disabled trigger's fire. The refusal
// claim takes only fires whose trigger is not active and names a schedule, and it reads the trigger, not the schedule:
// a refused fire never starts, so the schedule's state does not matter to it.
func TestTheStartClaimsRequireAnActiveTriggerAndTheRefusalClaimTakesOnlyInactiveOnes(t *testing.T) {
	ctx := context.Background()
	enabledTx := &schedGateClaimTx{}
	if _, err := (pgStartBatch{tx: enabledTx}).next(ctx, nil, time.Time{}); err != nil {
		t.Fatalf("next: %v", err)
	}
	disabledTx := &schedGateClaimTx{}
	if _, err := (pgStartBatch{tx: disabledTx}).nextDisabled(ctx, nil, time.Time{}); err != nil {
		t.Fatalf("nextDisabled: %v", err)
	}
	for _, c := range []struct {
		name string
		tx   *schedGateClaimTx
	}{{"next", enabledTx}, {"nextDisabled", disabledTx}} {
		if sql := strings.Join(strings.Fields(c.tx.sql), " "); !strings.Contains(sql, "t.status = 'active'") {
			t.Fatalf("%s does not require an active trigger; a paused trigger's fire would be started:\n%s", c.name, sql)
		}
	}

	inactiveTx := &schedGateClaimTx{}
	if _, err := (pgStartBatch{tx: inactiveTx}).nextInactive(ctx, nil); err != nil {
		t.Fatalf("nextInactive: %v", err)
	}
	sql := strings.Join(strings.Fields(inactiveTx.sql), " ")
	for _, want := range []string{
		"FROM event_trigger_fires f",
		"JOIN event_triggers t ON t.id = f.trigger_id",
		"f.status = 'authorized' AND f.task_id IS NULL",
		"t.status <> 'active' AND t.schedule_id IS NOT NULL",
		"FOR UPDATE OF f, t SKIP LOCKED",
	} {
		if !strings.Contains(sql, want) {
			t.Fatalf("nextInactive does not contain %q:\n%s", want, sql)
		}
	}
	if strings.Contains(sql, "cron_schedules") {
		t.Fatalf("nextInactive reads the schedule; a refused fire needs only its trigger:\n%s", sql)
	}
	if len(inactiveTx.args) != 1 {
		t.Fatalf("nextInactive binds %d arguments; want the excluded fire ids only", len(inactiveTx.args))
	}
	if _, ok := inactiveTx.args[0].([]string); !ok {
		t.Fatalf("nextInactive binds %T; want the excluded fire ids as a text array", inactiveTx.args[0])
	}
}

package eventtrigger

import (
	"context"
	"strings"
	"testing"
	"time"
)

// A soft-deleted schedule starts nothing. The starter refuses a fire of one schedule_deleted at the first pass that finds it,
// whatever the fire's age, and never leaves it authorised: the claims that start a run skip a deleted schedule, so without
// this refusal nothing would settle the fire. These tests drive the starter over the in-memory ledger and pin the claims with
// a capturing transaction. event_age_integration_test.go (TestDeletedScheduleFiresAreRefusedOnPostgres) pins the same
// behaviour against Postgres.

// schedGateDeletedFire is an authorised fire of an active trigger whose schedule is soft-deleted, authorised settledAgo before
// now.
func schedGateDeletedFire(fireID string, now time.Time, settledAgo time.Duration) *schedGateFire {
	f := windowedFire(fireID, true, now, settledAgo)
	f.deleted = true
	return f
}

// A fire of a soft-deleted schedule is refused schedule_deleted, writes no task, and is not left authorised, in the first pass
// and in every later one.
func TestStarterRefusesAFireWhoseScheduleIsSoftDeletedWithoutStartingIt(t *testing.T) {
	now := time.Now().UTC()
	f := schedGateDeletedFire("trgfire_deleted", now, time.Minute)
	l := &schedGateLedger{fires: []*schedGateFire{f}}
	for pass := 1; pass <= 2; pass++ {
		if started, err := windowedStarter(l, now).RunOnce(context.Background()); err != nil || started != 0 {
			t.Fatalf("pass %d: RunOnce = %d, %v; want no run started and no error", pass, started, err)
		}
	}
	if f.status != "refused" || f.outcome != OutcomeScheduleDeleted {
		t.Fatalf("fire is %s with outcome %q; want refused with %q", f.status, f.outcome, OutcomeScheduleDeleted)
	}
	if l.tasks != 0 {
		t.Fatalf("%d tasks written for a soft-deleted schedule; want none", l.tasks)
	}
}

// A soft-deleted schedule's fire is refused schedule_deleted whatever its age, and whether or not the schedule is also
// switched off. The trigger is checked first, so a paused trigger keeps trigger_not_active.
func TestAFireOfASoftDeletedScheduleIsRefusedWhateverItsAge(t *testing.T) {
	cases := []struct {
		name       string
		enabled    bool
		settledAgo time.Duration
		trigger    string
		outcome    string
	}{
		{name: "a fresh fire", enabled: true, settledAgo: time.Minute, trigger: "active", outcome: OutcomeScheduleDeleted},
		{name: "a fire past the window", enabled: true, settledAgo: 2 * fireStartWindow, trigger: "active",
			outcome: OutcomeScheduleDeleted},
		{name: "a switched-off schedule", enabled: false, settledAgo: time.Minute, trigger: "active",
			outcome: OutcomeScheduleDeleted},
		{name: "a fire of a paused trigger", enabled: true, settledAgo: time.Minute, trigger: "paused",
			outcome: OutcomeTriggerNotActive},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			now := time.Now().UTC()
			f := schedGateDeletedFire("trgfire_deleted_case", now, tc.settledAgo)
			f.enabled = tc.enabled
			f.triggerStatus = tc.trigger
			l := &schedGateLedger{fires: []*schedGateFire{f}}
			if _, err := windowedStarter(l, now).RunOnce(context.Background()); err != nil {
				t.Fatalf("RunOnce: %v", err)
			}
			if f.status != "refused" || f.outcome != tc.outcome || l.tasks != 0 {
				t.Fatalf("fire is %s with outcome %q and %d tasks; want refused with %q and no task",
					f.status, f.outcome, l.tasks, tc.outcome)
			}
		})
	}
}

// A fire of a live schedule still starts in the same pass that refuses a soft-deleted one, and only the live one takes a task.
func TestStarterStartsALiveFireAndRefusesADeletedOneInTheSamePass(t *testing.T) {
	now := time.Now().UTC()
	deleted := schedGateDeletedFire("trgfire_deleted_same", now, time.Minute)
	live := windowedFire("trgfire_live_same", true, now, time.Minute)
	l := &schedGateLedger{fires: []*schedGateFire{deleted, live}}
	started, err := windowedStarter(l, now).RunOnce(context.Background())
	if err != nil || started != 1 {
		t.Fatalf("RunOnce = %d, %v; want one run started and no error", started, err)
	}
	if live.status != "started" || live.taskID != "task_trgfire_live_same" {
		t.Fatalf("live fire is %s with task %q; want started with its task", live.status, live.taskID)
	}
	if deleted.status != "refused" || deleted.outcome != OutcomeScheduleDeleted {
		t.Fatalf("deleted fire is %s with outcome %q; want refused with %q", deleted.status, deleted.outcome, OutcomeScheduleDeleted)
	}
	if l.tasks != 1 {
		t.Fatalf("%d tasks written; want one, for the live schedule's fire", l.tasks)
	}
}

// The deleted-schedule claim takes only the fires of an active trigger whose schedule is soft-deleted. It binds no window,
// because the refusal does not depend on age, and it reads the schedule so the consent is checked against the schedule the
// fire names. It locks the rows it refuses, as the other claims lock theirs.
func TestTheDeletedScheduleClaimTakesOnlyDeletedSchedulesOfActiveTriggers(t *testing.T) {
	tx := &schedGateClaimTx{}
	if _, err := (pgStartBatch{tx: tx}).nextDeleted(context.Background(), nil); err != nil {
		t.Fatalf("nextDeleted: %v", err)
	}
	sql := strings.Join(strings.Fields(tx.sql), " ")
	for _, want := range []string{
		"FROM event_trigger_fires f",
		"JOIN event_triggers t ON t.id = f.trigger_id",
		"JOIN cron_schedules s ON s.id = t.schedule_id",
		"f.status = 'authorized' AND f.task_id IS NULL",
		"t.status = 'active' AND t.schedule_id IS NOT NULL",
		"s.deleted_at IS NOT NULL",
		"FOR UPDATE OF f, s, t SKIP LOCKED",
	} {
		if !strings.Contains(sql, want) {
			t.Fatalf("nextDeleted does not contain %q:\n%s", want, sql)
		}
	}
	if strings.Contains(sql, "$2") || strings.Contains(sql, "created_at") {
		t.Fatalf("nextDeleted binds a window; a deleted schedule's fire is refused whatever its age:\n%s", sql)
	}
	if len(tx.args) != 1 {
		t.Fatalf("nextDeleted binds %d arguments; want the excluded fire ids only", len(tx.args))
	}
	if _, ok := tx.args[0].([]string); !ok {
		t.Fatalf("nextDeleted binds %T; want the excluded fire ids as a text array", tx.args[0])
	}
}

// next and nextDisabled skip a soft-deleted schedule, which nextDeleted takes, so the two never claim the same fire.
func TestTheEnabledAndDisabledClaimsSkipASoftDeletedSchedule(t *testing.T) {
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
		if sql := strings.Join(strings.Fields(c.tx.sql), " "); !strings.Contains(sql, "s.deleted_at IS NULL") {
			t.Fatalf("%s does not skip a soft-deleted schedule; nextDeleted and %s would both claim its fire:\n%s", c.name, c.name, sql)
		}
	}
}

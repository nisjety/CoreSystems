package eventtrigger

import (
	"context"
	"strings"
	"testing"
	"time"
)

// The starter starts a run only for a fire authorised within fireStartWindow. The hourly budgets that admitted a fire count it
// in that window, so a run started after it would fall outside the budget, and a run switch turned on after a long pause must
// not start the runs of every event that piled up while it was off. A fire the starter reaches later is refused fire_expired,
// which is final, so the fire is never claimed again. These tests drive the starter over the in-memory ledger, whose claims
// apply the window as the Postgres claims do, and pin the claim queries with a capturing transaction.
// starter_fire_window_integration_test.go pins the same behaviour against Postgres.

// windowedFire is an authorised fire of an active trigger, authorised settledAgo before now, whose schedule has the given
// enabled state.
func windowedFire(fireID string, enabled bool, now time.Time, settledAgo time.Duration) *schedGateFire {
	f := schedGateAuthorised(fireID, enabled)
	f.settledAt = now.Add(-settledAgo)
	return f
}

// windowedStarter is a starter over the ledger whose clock reads now.
func windowedStarter(l starterLedger, now time.Time) *Starter {
	s := newStarter(l)
	s.now = func() time.Time { return now }
	return s
}

// A fire authorised before the window is refused fire_expired, writes no task, and stays refused in a later pass.
func TestStarterRefusesAFireAuthorisedBeforeTheWindowWithoutStartingIt(t *testing.T) {
	now := time.Now().UTC()
	f := windowedFire("trgfire_stale", true, now, fireStartWindow+time.Minute)
	l := &schedGateLedger{fires: []*schedGateFire{f}}
	s := windowedStarter(l, now)
	for pass := 1; pass <= 2; pass++ {
		if started, err := s.RunOnce(context.Background()); err != nil || started != 0 {
			t.Fatalf("pass %d: RunOnce = %d, %v; want no run started and no error", pass, started, err)
		}
	}
	if f.status != "refused" || f.outcome != OutcomeFireExpired {
		t.Fatalf("fire is %s with outcome %q; want refused with %q", f.status, f.outcome, OutcomeFireExpired)
	}
	if l.tasks != 0 {
		t.Fatalf("%d tasks written for a fire authorised before the window; want none", l.tasks)
	}
}

// A stale fire is refused as expired whatever its schedule's state, enabled or switched off. nextDisabled takes only the fires
// inside the window, so a switched-off schedule's stale fire is not reported as schedule_disabled.
func TestAStaleFireIsRefusedWhateverItsScheduleState(t *testing.T) {
	cases := []struct {
		name    string
		enabled bool
	}{
		{name: "an enabled schedule", enabled: true},
		{name: "a switched-off schedule", enabled: false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			now := time.Now().UTC()
			f := windowedFire("trgfire_stale_state", tc.enabled, now, 2*fireStartWindow)
			l := &schedGateLedger{fires: []*schedGateFire{f}}
			if _, err := windowedStarter(l, now).RunOnce(context.Background()); err != nil {
				t.Fatalf("RunOnce: %v", err)
			}
			if f.status != "refused" || f.outcome != OutcomeFireExpired || l.tasks != 0 {
				t.Fatalf("fire is %s with outcome %q and %d tasks; want refused with %q and no task",
					f.status, f.outcome, l.tasks, OutcomeFireExpired)
			}
		})
	}
}

// The window includes its edge: a fire authorised exactly fireStartWindow before the starter's clock still starts, and one
// nanosecond older is refused.
func TestTheStartWindowIncludesItsEdge(t *testing.T) {
	now := time.Now().UTC()

	edge := windowedFire("trgfire_edge", true, now, fireStartWindow)
	edgeLedger := &schedGateLedger{fires: []*schedGateFire{edge}}
	if started, err := windowedStarter(edgeLedger, now).RunOnce(context.Background()); err != nil || started != 1 {
		t.Fatalf("a fire on the window's edge: RunOnce = %d, %v; want one run started and no error", started, err)
	}
	if edge.status != "started" {
		t.Fatalf("a fire on the window's edge is %s; want started", edge.status)
	}

	older := windowedFire("trgfire_past_edge", true, now, fireStartWindow+time.Nanosecond)
	olderLedger := &schedGateLedger{fires: []*schedGateFire{older}}
	if started, err := windowedStarter(olderLedger, now).RunOnce(context.Background()); err != nil || started != 0 {
		t.Fatalf("a fire past the window's edge: RunOnce = %d, %v; want no run started and no error", started, err)
	}
	if older.status != "refused" || older.outcome != OutcomeFireExpired {
		t.Fatalf("a fire past the window's edge is %s with outcome %q; want refused with %q",
			older.status, older.outcome, OutcomeFireExpired)
	}
}

// A fire inside the window still starts in the pass that refuses an older fire queued ahead of it. The older fire takes no
// task, so a backlog of expired fires never holds back a fire that can start.
func TestAFreshFireStartsWhileAnOlderOneIsRefusedInTheSamePass(t *testing.T) {
	now := time.Now().UTC()
	older := windowedFire("trgfire_older_first", true, now, 2*fireStartWindow)
	fresh := windowedFire("trgfire_fresh_second", true, now, time.Minute)
	l := &schedGateLedger{fires: []*schedGateFire{older, fresh}}
	started, err := windowedStarter(l, now).RunOnce(context.Background())
	if err != nil || started != 1 {
		t.Fatalf("RunOnce = %d, %v; want one run started and no error", started, err)
	}
	if fresh.status != "started" || fresh.taskID != "task_trgfire_fresh_second" {
		t.Fatalf("fresh fire is %s with task %q; want started with its task", fresh.status, fresh.taskID)
	}
	if older.status != "refused" || older.outcome != OutcomeFireExpired {
		t.Fatalf("older fire is %s with outcome %q; want refused with %q", older.status, older.outcome, OutcomeFireExpired)
	}
	if l.tasks != 1 {
		t.Fatalf("%d tasks written; want one, for the fresh fire", l.tasks)
	}
}

// A stale fire of a paused trigger keeps trigger_not_active: the trigger is checked first, as the sweeper checks it.
func TestAStaleFireOfAPausedTriggerKeepsTriggerNotActive(t *testing.T) {
	now := time.Now().UTC()
	f := windowedFire("trgfire_stale_paused", true, now, 2*fireStartWindow)
	f.triggerStatus = "paused"
	l := &schedGateLedger{fires: []*schedGateFire{f}}
	if _, err := windowedStarter(l, now).RunOnce(context.Background()); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if f.status != "refused" || f.outcome != OutcomeTriggerNotActive || l.tasks != 0 {
		t.Fatalf("fire is %s with outcome %q and %d tasks; want refused with %q and no task",
			f.status, f.outcome, l.tasks, OutcomeTriggerNotActive)
	}
}

// The start claims bind the window as their third argument and share one query, so next and nextDisabled take only fires
// inside the window. nextExpired takes the older fires, binds the same bound as its second argument, reads only the trigger,
// and locks its rows as the other claims do.
func TestTheStartClaimsBindTheWindowAndTheExpiredClaimTakesOnlyOlderFires(t *testing.T) {
	ctx := context.Background()
	notBefore := time.Now().UTC().Add(-fireStartWindow)

	enabledTx := &schedGateClaimTx{}
	if _, err := (pgStartBatch{tx: enabledTx}).next(ctx, nil, notBefore); err != nil {
		t.Fatalf("next: %v", err)
	}
	disabledTx := &schedGateClaimTx{}
	if _, err := (pgStartBatch{tx: disabledTx}).nextDisabled(ctx, nil, notBefore); err != nil {
		t.Fatalf("nextDisabled: %v", err)
	}
	expiredTx := &schedGateClaimTx{}
	if _, err := (pgStartBatch{tx: expiredTx}).nextExpired(ctx, nil, notBefore); err != nil {
		t.Fatalf("nextExpired: %v", err)
	}

	if enabledTx.sql != disabledTx.sql {
		t.Fatalf("next and nextDisabled run different claims:\n%s\n---\n%s", enabledTx.sql, disabledTx.sql)
	}
	for _, c := range []struct {
		name  string
		tx    *schedGateClaimTx
		state bool
	}{{"next", enabledTx, true}, {"nextDisabled", disabledTx, false}} {
		sql := strings.Join(strings.Fields(c.tx.sql), " ")
		if !strings.Contains(sql, "f.settled_at >= $3") {
			t.Fatalf("%s does not bound the claim to the start window; an older fire would be started:\n%s", c.name, sql)
		}
		if len(c.tx.args) != 3 || c.tx.args[1] != c.state {
			t.Fatalf("%s binds %v; want the schedule state %t and the window", c.name, c.tx.args, c.state)
		}
		if got, ok := c.tx.args[2].(time.Time); !ok || !got.Equal(notBefore) {
			t.Fatalf("%s binds %v as the window; want %v", c.name, c.tx.args[2], notBefore)
		}
	}

	sql := strings.Join(strings.Fields(expiredTx.sql), " ")
	for _, want := range []string{
		"FROM event_trigger_fires f",
		"f.settled_at < $2",
		"t.status = 'active' AND t.schedule_id IS NOT NULL",
		"FOR UPDATE OF f, t SKIP LOCKED",
	} {
		if !strings.Contains(sql, want) {
			t.Fatalf("nextExpired does not contain %q:\n%s", want, sql)
		}
	}
	if strings.Contains(sql, "cron_schedules") {
		t.Fatalf("nextExpired reads the schedule; a refused fire needs only its trigger:\n%s", sql)
	}
	if len(expiredTx.args) != 2 {
		t.Fatalf("nextExpired binds %d arguments; want the excluded fire ids and the window", len(expiredTx.args))
	}
	if got, ok := expiredTx.args[1].(time.Time); !ok || !got.Equal(notBefore) {
		t.Fatalf("nextExpired binds %v as the window; want %v", expiredTx.args[1], notBefore)
	}
}

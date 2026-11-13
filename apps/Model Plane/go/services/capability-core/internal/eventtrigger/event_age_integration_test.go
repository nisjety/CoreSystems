//go:build integration

package eventtrigger

import (
	"context"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// The webhook event's own age and a schedule's soft deletion are decided in SQL and in the store, so they are checked against
// a real Postgres at each place they apply: intake records an event older than the window refused, the sweeper refuses a
// pending fire whose event has aged out without asking Control, and the starter refuses an authorised fire whose event has aged
// out, or whose schedule is soft-deleted. The scratch schema and its helpers are shared with
// starter_trigger_inactive_integration_test.go.
//
// Run with: go test -tags integration -run 'TestEventAgedOut|TestDeletedScheduleFires|TestEventIntakeOnPostgres' ./internal/eventtrigger/

// insertFireAt adds a fire of an event created at eventAt, which Control authorised at settledAt and for which no run has started.
func insertFireAt(t *testing.T, pool *pgxpool.Pool, id, triggerID, webhookEventID string, eventAt, settledAt time.Time) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO event_trigger_fires (id, trigger_id, org_id, webhook_event_id, status, outcome, attempts, created_at, settled_at)
		VALUES ($1, $2, $3, $4, 'authorized', 'authorized', 1, $5, $6)
	`, id, triggerID, inactiveOrg, webhookEventID, eventAt, settledAt); err != nil {
		t.Fatalf("insert fire %s: %v", id, err)
	}
}

// insertPendingFireAt adds a pending fire of an event created at eventAt.
func insertPendingFireAt(t *testing.T, pool *pgxpool.Pool, id, triggerID, webhookEventID string, eventAt time.Time) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO event_trigger_fires (id, trigger_id, org_id, webhook_event_id, status, outcome, attempts, created_at)
		VALUES ($1, $2, $3, $4, 'pending', '', 0, $5)
	`, id, triggerID, inactiveOrg, webhookEventID, eventAt); err != nil {
		t.Fatalf("insert pending fire %s: %v", id, err)
	}
}

// assertEventFire reads the fire of one webhook event and checks its status, outcome, creation time and whether it is settled.
func assertEventFire(t *testing.T, pool *pgxpool.Pool, webhookEventID, wantStatus, wantOutcome string, wantCreated time.Time, wantSettled bool) {
	t.Helper()
	var status, outcome string
	var createdAt time.Time
	var settledAt *time.Time
	if err := pool.QueryRow(context.Background(), `
		SELECT status, outcome, created_at, settled_at FROM event_trigger_fires WHERE webhook_event_id = $1`, webhookEventID).
		Scan(&status, &outcome, &createdAt, &settledAt); err != nil {
		t.Fatalf("read the fire of %s: %v", webhookEventID, err)
	}
	if status != wantStatus || outcome != wantOutcome || (settledAt != nil) != wantSettled {
		t.Fatalf("the fire of %s is %s with outcome %q settled %v; want %s with outcome %q settled %t",
			webhookEventID, status, outcome, settledAt != nil, wantStatus, wantOutcome, wantSettled)
	}
	// Postgres keeps microseconds, so the event's nanoseconds do not survive the write.
	if !createdAt.Equal(wantCreated.Truncate(time.Microsecond)) {
		t.Fatalf("the fire of %s was created at %v; want its event's time %v", webhookEventID, createdAt, wantCreated)
	}
}

// An authorised fire whose event is older than the window is refused fire_expired and writes no task, although Control
// authorised it inside the window. A fire whose event is fresh still starts in the same pass.
func TestEventAgedOutFiresAreRefusedOnPostgres(t *testing.T) {
	ctx := context.Background()
	pool := setupInactiveFireDB(t)
	insertInactiveSchedule(t, pool, "sched_aged")
	insertInactiveTrigger(t, pool, "trg_aged", "active", "sched_aged")

	now := time.Now().UTC()
	insertFireAt(t, pool, "trgfire_aged", "trg_aged", "evt_aged", now.Add(-fireWindow-time.Minute), now.Add(-time.Minute))
	insertFireAt(t, pool, "trgfire_event_fresh", "trg_aged", "evt_event_fresh", now.Add(-time.Minute), now.Add(-time.Minute))

	if started, err := windowedStarter(pgStarterStore{pool: pool}, now).RunOnce(ctx); err != nil || started != 1 {
		t.Fatalf("pass: RunOnce = %d, %v; want one run started (the fresh event) and no error", started, err)
	}
	assertInactiveFire(t, pool, "trgfire_aged", "refused", OutcomeFireExpired, false)
	assertInactiveFire(t, pool, "trgfire_event_fresh", "authorized", "authorized", true)
	if n := countInactiveTasks(t, pool); n != 1 {
		t.Fatalf("%d tasks written; want one, for the fresh event only", n)
	}
}

// A fire of a soft-deleted schedule is refused schedule_deleted and writes no task, while a fire of a live schedule still starts
// in the same pass. A later pass starts nothing more.
func TestDeletedScheduleFiresAreRefusedOnPostgres(t *testing.T) {
	ctx := context.Background()
	pool := setupInactiveFireDB(t)
	insertInactiveSchedule(t, pool, "sched_deleted")
	insertInactiveSchedule(t, pool, "sched_live")
	insertInactiveTrigger(t, pool, "trg_deleted", "active", "sched_deleted")
	insertInactiveTrigger(t, pool, "trg_live", "active", "sched_live")
	if _, err := pool.Exec(ctx, `UPDATE cron_schedules SET deleted_at = now() WHERE id = $1`, "sched_deleted"); err != nil {
		t.Fatalf("soft-delete the schedule: %v", err)
	}

	now := time.Now().UTC()
	insertAuthorisedFire(t, pool, "trgfire_deleted", "trg_deleted", "evt_deleted")
	insertAuthorisedFire(t, pool, "trgfire_live", "trg_live", "evt_live")

	if started, err := windowedStarter(pgStarterStore{pool: pool}, now).RunOnce(ctx); err != nil || started != 1 {
		t.Fatalf("pass: RunOnce = %d, %v; want one run started (the live schedule) and no error", started, err)
	}
	assertInactiveFire(t, pool, "trgfire_deleted", "refused", OutcomeScheduleDeleted, false)
	assertInactiveFire(t, pool, "trgfire_live", "authorized", "authorized", true)
	if n := countInactiveTasks(t, pool); n != 1 {
		t.Fatalf("%d tasks written; want one, for the live schedule's fire only", n)
	}

	if started, err := windowedStarter(pgStarterStore{pool: pool}, now).RunOnce(ctx); err != nil || started != 0 {
		t.Fatalf("second pass: RunOnce = %d, %v; want no run started and no error", started, err)
	}
	assertInactiveFire(t, pool, "trgfire_deleted", "refused", OutcomeScheduleDeleted, false)
}

// Intake records an event already older than the window refused fire_expired, and a fresh event pending with its own creation
// time; a redelivery of the fresh event writes nothing. The sweeper then refuses a pending fire whose event has aged out since
// intake without asking Control, and authorises the fresh ones.
func TestEventIntakeOnPostgres(t *testing.T) {
	ctx := context.Background()
	pool := setupInactiveFireDB(t)
	insertInactiveSchedule(t, pool, "sched_intake")
	insertInactiveTrigger(t, pool, "trg_intake", "active", "sched_intake")
	store := pgStore{pool: pool}
	now := time.Now().UTC()

	oldEvent := WebhookReceived{OrgID: inactiveOrg, ConnectionID: "conn_inactive", WebhookEventID: "evt_intake_old",
		EventAt: now.Add(-fireWindow - time.Minute)}
	if n, err := store.record(ctx, oldEvent, now); err != nil || n != 1 {
		t.Fatalf("record an old event = %d, %v; want one fire recorded", n, err)
	}
	freshEvent := WebhookReceived{OrgID: inactiveOrg, ConnectionID: "conn_inactive", WebhookEventID: "evt_intake_fresh",
		EventAt: now.Add(-time.Minute)}
	if n, err := store.record(ctx, freshEvent, now); err != nil || n != 1 {
		t.Fatalf("record a fresh event = %d, %v; want one fire recorded", n, err)
	}
	if n, err := store.record(ctx, freshEvent, now); err != nil || n != 0 {
		t.Fatalf("redeliver the fresh event = %d, %v; want no fire recorded", n, err)
	}
	assertEventFire(t, pool, "evt_intake_old", "refused", OutcomeFireExpired, oldEvent.EventAt, true)
	assertEventFire(t, pool, "evt_intake_fresh", "pending", "", freshEvent.EventAt, false)

	// The sweeper: a pending fire whose event has aged out is refused without a question to Control, and the fresh one is
	// authorised. The fresh intake fire is pending too, so Control is asked about both fresh events and no other.
	insertPendingFireAt(t, pool, "trgfire_sweep_aged", "trg_intake", "evt_sweep_aged", now.Add(-fireWindow-time.Minute))
	insertPendingFireAt(t, pool, "trgfire_sweep_fresh", "trg_intake", "evt_sweep_fresh", now.Add(-time.Minute))
	authority := &fakeAuthorizer{}
	sweeper := newSweeper(store, authority, testLimits)
	sweeper.now = func() time.Time { return now }
	if authorized, err := sweeper.RunOnce(ctx); err != nil || authorized != 2 {
		t.Fatalf("sweep = %d, %v; want the two fresh events authorised and no error", authorized, err)
	}
	for _, intent := range authority.intents {
		if intent.FireKey == "evt_sweep_aged" || intent.FireKey == "evt_intake_old" {
			t.Fatalf("Control was asked about an event older than the window: %+v", intent)
		}
	}
	if len(authority.intents) != 2 {
		t.Fatalf("Control was asked %d times; want 2, for the fresh events only", len(authority.intents))
	}
	assertEventFire(t, pool, "evt_sweep_aged", "refused", OutcomeFireExpired, now.Add(-fireWindow-time.Minute), true)
	assertEventFire(t, pool, "evt_sweep_fresh", "authorized", OutcomeAuthorized, now.Add(-time.Minute), true)
	assertEventFire(t, pool, "evt_intake_fresh", "authorized", OutcomeAuthorized, freshEvent.EventAt, true)
}

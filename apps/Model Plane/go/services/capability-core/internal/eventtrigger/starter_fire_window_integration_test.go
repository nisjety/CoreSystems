//go:build integration

package eventtrigger

import (
	"context"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// A fire authorised before the start window is refused fire_expired against a real Postgres, while a fresh fire of the same
// trigger starts a run in the same pass. The scratch schema and its helpers are shared with
// starter_trigger_inactive_integration_test.go, so the window is read through the claims the starter runs in production.
//
// Run with: go test -tags integration -run TestStaleFiresAreRefusedOnPostgres ./internal/eventtrigger/

// insertFireAuthorisedAt adds a fire that Control authorised at settledAt, for which no run has started.
func insertFireAuthorisedAt(t *testing.T, pool *pgxpool.Pool, id, triggerID, webhookEventID string, settledAt time.Time) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO event_trigger_fires (id, trigger_id, org_id, webhook_event_id, status, outcome, attempts, settled_at)
		VALUES ($1, $2, $3, $4, 'authorized', 'authorized', 1, $5)
	`, id, triggerID, inactiveOrg, webhookEventID, settledAt); err != nil {
		t.Fatalf("insert fire %s: %v", id, err)
	}
}

// A fire authorised before the window is refused fire_expired and writes no task, while the fresh fire of the same trigger
// still starts. A later pass starts nothing more.
func TestStaleFiresAreRefusedOnPostgres(t *testing.T) {
	ctx := context.Background()
	pool := setupInactiveFireDB(t)
	insertInactiveSchedule(t, pool, "sched_window")
	insertInactiveTrigger(t, pool, "trg_window", "active", "sched_window")

	now := time.Now().UTC()
	insertFireAuthorisedAt(t, pool, "trgfire_window_stale", "trg_window", "evt_window_stale", now.Add(-2*fireStartWindow))
	insertFireAuthorisedAt(t, pool, "trgfire_window_fresh", "trg_window", "evt_window_fresh", now.Add(-time.Minute))

	if started, err := windowedStarter(pgStarterStore{pool: pool}, now).RunOnce(ctx); err != nil || started != 1 {
		t.Fatalf("pass: RunOnce = %d, %v; want one run started (the fresh fire) and no error", started, err)
	}
	assertInactiveFire(t, pool, "trgfire_window_stale", "refused", OutcomeFireExpired, false)
	assertInactiveFire(t, pool, "trgfire_window_fresh", "authorized", "authorized", true)
	if n := countInactiveTasks(t, pool); n != 1 {
		t.Fatalf("%d tasks written; want one, for the fresh fire only", n)
	}

	// A later pass starts nothing more: the refused fire is settled, and the started fire already has its task.
	if started, err := windowedStarter(pgStarterStore{pool: pool}, now).RunOnce(ctx); err != nil || started != 0 {
		t.Fatalf("second pass: RunOnce = %d, %v; want no run started and no error", started, err)
	}
	assertInactiveFire(t, pool, "trgfire_window_stale", "refused", OutcomeFireExpired, false)
	if n := countInactiveTasks(t, pool); n != 1 {
		t.Fatalf("%d tasks after the second pass; want still one", n)
	}
}

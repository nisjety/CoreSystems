//go:build integration

package eventtrigger

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
)

// The pause the starter must honour is a row change, and the claim that refuses the fire is SQL, so this test runs the
// starter against a real Postgres. The scratch schema carries the columns the claims, the refusal, the intake
// (pgStore.record) and cron.InsertFire read and write: the session-core tables event_triggers and event_trigger_fires
// (migrations 0113, 0114, 0118 and 0119) and the cron tables as the cron integration test declares them. event_triggers
// has the created_at column migration 0113 declares, which the intake's trigger match orders by.
//
// Run with: go test -tags integration -run TestPausedAndDisabledTriggerFiresAreRefusedOnPostgres
//   ./internal/eventtrigger/

const inactiveFireSchema = `
CREATE TABLE cron_schedules (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL DEFAULT '',
    space_ref TEXT NOT NULL DEFAULT '',
    creator_subject_id TEXT NOT NULL DEFAULT '',
    template_digest TEXT NOT NULL DEFAULT '',
    schedule_expr TEXT NOT NULL,
    timezone TEXT NOT NULL DEFAULT 'UTC',
    task_template JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    deleted_at TIMESTAMPTZ
);
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL DEFAULT 'agent',
    title TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    assignee TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'created',
    priority INT NOT NULL DEFAULT 0,
    config_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    scheduled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE cron_fires (
    id TEXT PRIMARY KEY,
    schedule_id TEXT NOT NULL,
    task_id TEXT,
    fired_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    status TEXT NOT NULL DEFAULT 'pending',
    error TEXT,
    fire_key TEXT NOT NULL DEFAULT ''
);
CREATE TABLE event_triggers (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    space_ref TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    member_user_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'disabled')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    schedule_id TEXT NULL
);
CREATE TABLE event_trigger_fires (
    id TEXT PRIMARY KEY,
    trigger_id TEXT NOT NULL REFERENCES event_triggers(id) ON DELETE CASCADE,
    org_id TEXT NOT NULL,
    webhook_event_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'authorized', 'refused')),
    outcome TEXT NOT NULL DEFAULT '',
    attempts INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    settled_at TIMESTAMPTZ NULL,
    task_id TEXT NULL,
    started_at TIMESTAMPTZ NULL,
    CONSTRAINT event_trigger_fires_once UNIQUE (trigger_id, webhook_event_id),
    CONSTRAINT event_trigger_fires_settled CHECK ((status = 'pending') = (settled_at IS NULL))
);
`

const (
	inactiveOrg   = "org_inactive"
	inactiveSpace = "space_inactive"
	inactiveUser  = "user_inactive"
)

func setupInactiveFireDB(t *testing.T) *pgxpool.Pool {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("event_trigger_inactive"),
		tcpostgres.WithUsername("test"),
		tcpostgres.WithPassword("test"),
		testcontainers.WithWaitStrategy(
			wait.ForLog("database system is ready to accept connections").
				WithOccurrence(2).WithStartupTimeout(60*time.Second),
		),
	)
	if err != nil {
		t.Fatalf("start postgres: %v", err)
	}
	t.Cleanup(func() {
		c, cc := context.WithTimeout(context.Background(), 30*time.Second)
		defer cc()
		_ = container.Terminate(c)
	})
	dsn, err := container.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("connection string: %v", err)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect postgres: %v", err)
	}
	t.Cleanup(pool.Close)
	if _, err := pool.Exec(ctx, inactiveFireSchema); err != nil {
		t.Fatalf("create scratch schema: %v", err)
	}
	return pool
}

// insertInactiveSchedule adds an enabled schedule of the test member's org and Space.
func insertInactiveSchedule(t *testing.T, pool *pgxpool.Pool, id string) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO cron_schedules (id, org_id, space_ref, creator_subject_id, template_digest,
		    schedule_expr, timezone, task_template, enabled)
		VALUES ($1, $2, $3, $4, $5, '0 0 1 1 *', 'UTC', $6::jsonb, true)
	`, id, inactiveOrg, inactiveSpace, inactiveUser, "sha256:"+strings.Repeat("b", 64),
		`{"kind":"cron","title":"Event run"}`); err != nil {
		t.Fatalf("insert schedule %s: %v", id, err)
	}
}

// insertInactiveTrigger adds a trigger of the test member that names the schedule, in the given status.
func insertInactiveTrigger(t *testing.T, pool *pgxpool.Pool, id, status, scheduleID string) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO event_triggers (id, org_id, space_ref, connection_id, member_user_id,
		    event_type, name, status, schedule_id)
		VALUES ($1, $2, $3, 'conn_inactive', $4, 'webhook_received', 'Event trigger', $5, $6)
	`, id, inactiveOrg, inactiveSpace, inactiveUser, status, scheduleID); err != nil {
		t.Fatalf("insert trigger %s: %v", id, err)
	}
}

// insertAuthorisedFire adds a fire that Control has authorised and no run has started for.
func insertAuthorisedFire(t *testing.T, pool *pgxpool.Pool, id, triggerID, webhookEventID string) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO event_trigger_fires (id, trigger_id, org_id, webhook_event_id, status, outcome, attempts, settled_at)
		VALUES ($1, $2, $3, $4, 'authorized', 'authorized', 1, now())
	`, id, triggerID, inactiveOrg, webhookEventID); err != nil {
		t.Fatalf("insert fire %s: %v", id, err)
	}
}

// assertInactiveFire reads one fire and checks its status, its outcome, and whether a task was recorded on it.
func assertInactiveFire(t *testing.T, pool *pgxpool.Pool, id, wantStatus, wantOutcome string, wantTask bool) {
	t.Helper()
	var status, outcome string
	var taskID *string
	if err := pool.QueryRow(context.Background(),
		`SELECT status, outcome, task_id FROM event_trigger_fires WHERE id = $1`, id).
		Scan(&status, &outcome, &taskID); err != nil {
		t.Fatalf("read fire %s: %v", id, err)
	}
	if status != wantStatus || outcome != wantOutcome || (taskID != nil) != wantTask {
		t.Fatalf("fire %s is %s with outcome %q and task %v; want %s with outcome %q and a task: %v",
			id, status, outcome, taskID, wantStatus, wantOutcome, wantTask)
	}
}

func countInactiveTasks(t *testing.T, pool *pgxpool.Pool) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(context.Background(), `SELECT count(*) FROM tasks`).Scan(&n); err != nil {
		t.Fatalf("count tasks: %v", err)
	}
	return n
}

// A paused or disabled trigger's authorised fire is refused by the first pass that finds the trigger inactive, and a
// re-activation starts nothing for it. A fire of an active trigger still starts through the same starter.
func TestPausedAndDisabledTriggerFiresAreRefusedOnPostgres(t *testing.T) {
	ctx := context.Background()
	pool := setupInactiveFireDB(t)
	insertInactiveSchedule(t, pool, "sched_inactive")
	starter := newStarter(pgStarterStore{pool: pool})

	insertInactiveTrigger(t, pool, "trg_paused", "paused", "sched_inactive")
	insertAuthorisedFire(t, pool, "trgfire_paused", "trg_paused", "evt_paused")
	if started, err := starter.RunOnce(ctx); err != nil || started != 0 {
		t.Fatalf("pass while paused: RunOnce = %d, %v; want no run started and no error", started, err)
	}
	assertInactiveFire(t, pool, "trgfire_paused", "refused", OutcomeTriggerNotActive, false)

	// The owner re-activates the trigger: the refused fire must not start the old event's run.
	if _, err := pool.Exec(ctx, `UPDATE event_triggers SET status = 'active' WHERE id = $1`, "trg_paused"); err != nil {
		t.Fatalf("re-activate trigger: %v", err)
	}
	if started, err := starter.RunOnce(ctx); err != nil || started != 0 {
		t.Fatalf("pass after re-activation: RunOnce = %d, %v; want no run started and no error", started, err)
	}
	assertInactiveFire(t, pool, "trgfire_paused", "refused", OutcomeTriggerNotActive, false)

	insertInactiveTrigger(t, pool, "trg_disabled", "disabled", "sched_inactive")
	insertAuthorisedFire(t, pool, "trgfire_disabled", "trg_disabled", "evt_disabled")
	if started, err := starter.RunOnce(ctx); err != nil || started != 0 {
		t.Fatalf("pass for a disabled trigger: RunOnce = %d, %v; want no run started and no error", started, err)
	}
	assertInactiveFire(t, pool, "trgfire_disabled", "refused", OutcomeTriggerNotActive, false)
	if n := countInactiveTasks(t, pool); n != 0 {
		t.Fatalf("%d tasks written for inactive triggers; want none", n)
	}

	// Control: an active trigger's fire still starts a run, so the refusal claim does not take it.
	insertInactiveTrigger(t, pool, "trg_active", "active", "sched_inactive")
	insertAuthorisedFire(t, pool, "trgfire_active", "trg_active", "evt_active")
	if started, err := starter.RunOnce(ctx); err != nil || started != 1 {
		t.Fatalf("pass for an active trigger: RunOnce = %d, %v; want one run started and no error", started, err)
	}
	assertInactiveFire(t, pool, "trgfire_active", "authorized", "authorized", true)
	if n := countInactiveTasks(t, pool); n != 1 {
		t.Fatalf("%d tasks written; want one, for the active trigger's fire only", n)
	}
}

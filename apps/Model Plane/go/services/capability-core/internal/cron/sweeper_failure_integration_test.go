//go:build integration

package cron

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

// The failure isolated here is Postgres's own rule that one failed statement aborts
// the whole transaction until a savepoint is rolled back. The constraints that make
// a fire fail are enforced only by a real table, so this test needs a real Postgres.
//
// The scratch schema carries the columns RunOnce and fireOne write, with the column
// types of session-core migrations 0004 (tasks, cron_schedules, cron_fires), 0028
// (cron_schedules' space and template columns) and 0056 (cron_fires' fire_key).
// Two named CHECK constraints make a fire fail for one row only: tasks_no_poison
// refuses a task titled "poison", and cron_fires_no_stuck refuses fire rows of the
// schedule "sched-stuck".
//
// Run with: go test -tags integration -run TestFailedFireIsIsolatedInItsBatch ./internal/cron/

const sweepFailureSchema = `
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
    last_fire_at TIMESTAMPTZ,
    next_fire_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
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
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT tasks_no_poison CHECK (title <> 'poison')
);
CREATE TABLE cron_fires (
    id TEXT PRIMARY KEY,
    schedule_id TEXT NOT NULL,
    task_id TEXT,
    fired_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    status TEXT NOT NULL DEFAULT 'pending',
    error TEXT,
    fire_key TEXT NOT NULL DEFAULT '',
    CONSTRAINT cron_fires_no_stuck CHECK (schedule_id <> 'sched-stuck')
);
`

type allowEveryFire struct{}

func (allowEveryFire) AuthorizeFire(context.Context, FireIntent) error { return nil }

func setupSweepFailureDB(t *testing.T) *pgxpool.Pool {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("sweep_failure"),
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
	if _, err := pool.Exec(ctx, sweepFailureSchema); err != nil {
		t.Fatalf("create scratch schema: %v", err)
	}
	return pool
}

// insertDueSchedule adds an enabled schedule whose slot is overdue by the given
// amount. Its expression fires once a year, so no schedule in this test comes due
// again while the test runs.
func insertDueSchedule(t *testing.T, pool *pgxpool.Pool, id, orgID, title string, overdue time.Duration) {
	t.Helper()
	template := `{"kind":"cron","title":"` + title + `"}`
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO cron_schedules (id, org_id, space_ref, creator_subject_id, template_digest,
		    schedule_expr, timezone, task_template, enabled, next_fire_at)
		VALUES ($1, $2, 'space-1', 'user-1', $3, '0 0 1 1 *', 'UTC', $4::jsonb, true, $5)
	`, id, orgID, "sha256:"+strings.Repeat("a", 64), template, time.Now().UTC().Add(-overdue)); err != nil {
		t.Fatalf("insert schedule %s: %v", id, err)
	}
}

// slotOf returns the fire key the sweeper derives from a schedule's next_fire_at.
func slotOf(t *testing.T, pool *pgxpool.Pool, id string) string {
	t.Helper()
	var due time.Time
	if err := pool.QueryRow(context.Background(),
		`SELECT next_fire_at FROM cron_schedules WHERE id = $1`, id).Scan(&due); err != nil {
		t.Fatalf("read slot of %s: %v", id, err)
	}
	return due.UTC().Format(time.RFC3339Nano)
}

func countRows(t *testing.T, pool *pgxpool.Pool, query string, args ...any) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(context.Background(), query, args...).Scan(&n); err != nil {
		t.Fatalf("count query %q: %v", query, err)
	}
	return n
}

// assertFireRow checks that a schedule has exactly one cron_fires row with the given
// status and fire key, and that its error names wantErr when one is given.
func assertFireRow(t *testing.T, pool *pgxpool.Pool, scheduleID, wantStatus, wantKey, wantErr string) {
	t.Helper()
	if n := countRows(t, pool, `SELECT count(*) FROM cron_fires WHERE schedule_id = $1`, scheduleID); n != 1 {
		t.Fatalf("%s has %d cron_fires rows, want 1", scheduleID, n)
	}
	var status, errText, fireKey string
	if err := pool.QueryRow(context.Background(),
		`SELECT status, COALESCE(error, ''), fire_key FROM cron_fires WHERE schedule_id = $1`,
		scheduleID).Scan(&status, &errText, &fireKey); err != nil {
		t.Fatalf("read fire row of %s: %v", scheduleID, err)
	}
	if status != wantStatus || fireKey != wantKey {
		t.Fatalf("%s fire row has status %q and key %q, want %q and %q",
			scheduleID, status, fireKey, wantStatus, wantKey)
	}
	if wantErr != "" && !strings.Contains(errText, wantErr) {
		t.Fatalf("%s fire row error %q does not name %q", scheduleID, errText, wantErr)
	}
}

// assertMovedPastSlot checks that a schedule's slot has advanced and that the
// schedule is still enabled and has a last fire time.
func assertMovedPastSlot(t *testing.T, pool *pgxpool.Pool, id string) {
	t.Helper()
	var future, hasLastFire, enabled bool
	if err := pool.QueryRow(context.Background(), `
		SELECT next_fire_at > now(), last_fire_at IS NOT NULL, enabled
		FROM cron_schedules WHERE id = $1`, id).Scan(&future, &hasLastFire, &enabled); err != nil {
		t.Fatalf("read schedule %s: %v", id, err)
	}
	if !future || !hasLastFire || !enabled {
		t.Fatalf("%s: next slot in the future = %v, last_fire_at set = %v, enabled = %v; want all true",
			id, future, hasLastFire, enabled)
	}
}

func TestFailedFireIsIsolatedInItsBatch(t *testing.T) {
	pool := setupSweepFailureDB(t)
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	sweeper := NewSweeper(pool, allowEveryFire{})

	// sched-bad is first in the batch (ORDER BY next_fire_at NULLS FIRST). Its failing
	// task insert used to abort the shared transaction before sched-good could fire.
	insertDueSchedule(t, pool, "sched-bad", "org-bad", "poison", 2*time.Minute)
	insertDueSchedule(t, pool, "sched-good", "org-good", "fine", time.Minute)
	badSlot := slotOf(t, pool, "sched-bad")
	goodSlot := slotOf(t, pool, "sched-good")

	fired, err := sweeper.RunOnce(ctx)
	if fired != 1 {
		t.Fatalf("fired = %d, want 1: the healthy schedule must commit despite its failing batch-mate (err = %v)", fired, err)
	}
	for _, want := range []string{"sched-bad", "org-bad", "tasks_no_poison"} {
		if err == nil || !strings.Contains(err.Error(), want) {
			t.Fatalf("RunOnce error = %v, want it to name %q", err, want)
		}
	}
	if n := countRows(t, pool, `SELECT count(*) FROM tasks WHERE org_id = 'org-bad'`); n != 0 {
		t.Fatalf("the failed fire left %d task rows, want 0", n)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM tasks WHERE org_id = 'org-good' AND title = 'fine'`); n != 1 {
		t.Fatalf("the healthy fire left %d task rows, want 1", n)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM cron_fires WHERE schedule_id = 'sched-good' AND status = 'pending' AND task_id IS NOT NULL`); n != 1 {
		t.Fatalf("the healthy fire has %d pending rows with a task, want 1", n)
	}
	assertFireRow(t, pool, "sched-good", "pending", goodSlot, "")
	assertFireRow(t, pool, "sched-bad", "failed", badSlot, "tasks_no_poison")
	assertMovedPastSlot(t, pool, "sched-good")
	assertMovedPastSlot(t, pool, "sched-bad")

	// Both slots are consumed, so the next tick has nothing to do.
	fired, err = sweeper.RunOnce(ctx)
	if fired != 0 || err != nil {
		t.Fatalf("second tick fired %d, err = %v; want nothing due", fired, err)
	}

	// A failed fire that cannot be recorded either stays due, so the next tick
	// retries it, while the healthy schedule in the same batch still commits.
	insertDueSchedule(t, pool, "sched-stuck", "org-stuck", "poison", 3*time.Minute)
	insertDueSchedule(t, pool, "sched-good-2", "org-good", "fine", time.Minute)
	stuckSlot := slotOf(t, pool, "sched-stuck")
	fired, err = sweeper.RunOnce(ctx)
	if fired != 1 {
		t.Fatalf("fired = %d, want 1 when one fire cannot be recorded (err = %v)", fired, err)
	}
	for _, want := range []string{"sched-stuck", "tasks_no_poison", "cron_fires_no_stuck"} {
		if err == nil || !strings.Contains(err.Error(), want) {
			t.Fatalf("RunOnce error = %v, want it to name %q", err, want)
		}
	}
	if got := slotOf(t, pool, "sched-stuck"); got != stuckSlot {
		t.Fatalf("sched-stuck moved from slot %s to %s; an unrecorded failure must leave it due", stuckSlot, got)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM tasks WHERE org_id = 'org-stuck'`); n != 0 {
		t.Fatalf("the unrecorded failure left %d task rows, want 0", n)
	}
	if n := countRows(t, pool, `SELECT count(*) FROM tasks WHERE org_id = 'org-good' AND title = 'fine'`); n != 2 {
		t.Fatalf("the healthy fires left %d task rows in total, want 2", n)
	}
	fired, err = sweeper.RunOnce(ctx)
	if fired != 0 || err == nil || !strings.Contains(err.Error(), "sched-stuck") {
		t.Fatalf("retry tick fired %d, err = %v; want no fire and an error naming sched-stuck", fired, err)
	}
}

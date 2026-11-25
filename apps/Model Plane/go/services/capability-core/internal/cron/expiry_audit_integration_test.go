//go:build integration

package cron

import (
	"context"
	"errors"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
)

// The expiry row of a guard pause (K3-4, P2-go-08) is pinned here on a real Postgres, through Sweeper.RunOnce,
// because two of its rules are enforced only by a real transaction: a row lost with the process is written by
// the sweep's durable recovery, and the row of a pause whose batch fails at COMMIT is never written.
//
// The scratch schema carries the columns RunOnce, pauseSchedule and the recovery use, with the column types of
// session-core migrations 0004 (tasks, cron_schedules, cron_fires), 0028 (cron_schedules' space and template
// columns) and 0056 (cron_fires' fire_key and outcome), and the registry_audit_log of capability-core
// migration 0003.
//
// Run with: go test -tags integration -run TestExpiryAudit ./internal/cron/

const expiryAuditSchema = `
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
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE cron_fires (
    id TEXT PRIMARY KEY,
    schedule_id TEXT NOT NULL REFERENCES cron_schedules(id),
    task_id TEXT REFERENCES tasks(id),
    fired_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    status TEXT NOT NULL DEFAULT 'pending',
    error TEXT NOT NULL DEFAULT '',
    completed_at TIMESTAMPTZ,
    fire_key TEXT NOT NULL DEFAULT '',
    run_id TEXT NOT NULL DEFAULT '',
    thread_id TEXT NOT NULL DEFAULT '',
    withheld_effects INTEGER NOT NULL DEFAULT 0,
    outcome TEXT NOT NULL DEFAULT ''
);
CREATE TABLE registry_audit_log (
    id TEXT PRIMARY KEY,
    entity_kind TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    action TEXT NOT NULL,
    actor TEXT NOT NULL DEFAULT '',
    org_id TEXT NOT NULL DEFAULT '',
    diff_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    ts TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE FUNCTION refuse_expired_pause_at_commit() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'commit refused for a paused_expired fire';
END
$$;
`

// refuseExpiredPauseAtCommit makes every batch that holds a paused_expired fire fail at its COMMIT. The trigger
// is deferred, so the pause's own statements and savepoint succeed and only the commit fails.
const refuseExpiredPauseAtCommit = `
CREATE CONSTRAINT TRIGGER cron_fires_refuse_expired_pause
    AFTER INSERT ON cron_fires
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW WHEN (NEW.outcome = 'paused_expired')
    EXECUTE FUNCTION refuse_expired_pause_at_commit()
`

func setupExpiryAuditDB(t *testing.T) *pgxpool.Pool {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("expiry_audit"),
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
	if _, err := pool.Exec(ctx, expiryAuditSchema); err != nil {
		t.Fatalf("create scratch schema: %v", err)
	}
	return pool
}

// resetExpiryAuditDB empties every table, so each subtest starts from no schedules and no audit rows.
func resetExpiryAuditDB(t *testing.T, pool *pgxpool.Pool) {
	t.Helper()
	if _, err := pool.Exec(context.Background(),
		`TRUNCATE registry_audit_log, cron_fires, tasks, cron_schedules`); err != nil {
		t.Fatalf("reset scratch schema: %v", err)
	}
}

// insertLapsedSchedule adds an enabled schedule whose slot is a minute overdue and whose approval expired an hour
// ago, so the next sweep pauses it under its approval guard.
func insertLapsedSchedule(t *testing.T, pool *pgxpool.Pool, id, orgID string) {
	t.Helper()
	now := time.Now().UTC()
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO cron_schedules (id, org_id, space_ref, creator_subject_id, template_digest,
		    schedule_expr, timezone, task_template, enabled, next_fire_at)
		VALUES ($1, $2, 'space-1', 'user-1', $3, '0 0 1 1 *', 'UTC', $4::jsonb, true, $5)
	`, id, orgID, "sha256:"+strings.Repeat("a", 64), string(guardTemplate(t, now.Add(-time.Hour))), now.Add(-time.Minute)); err != nil {
		t.Fatalf("insert schedule %s: %v", id, err)
	}
}

// expiredRowRefusingAuditor writes through the real registry_audit_log writer, except that it refuses every
// schedule.contract.expired row while refuse is set, as a registry that drops that one write would.
type expiredRowRefusingAuditor struct {
	inner   ContractAuditor
	refuse  atomic.Bool
	refused atomic.Int32
}

func (a *expiredRowRefusingAuditor) Append(ctx context.Context, db AuditExecer, row ContractAuditRow) error {
	if row.Action == ContractAuditExpired && a.refuse.Load() {
		a.refused.Add(1)
		return errors.New("registry_audit_log refused the expiry row")
	}
	return a.inner.Append(ctx, db, row)
}

func newExpiredRowRefusingAuditor(t *testing.T, pool *pgxpool.Pool) *expiredRowRefusingAuditor {
	t.Helper()
	inner, err := NewContractAuditor(pool)
	if err != nil {
		t.Fatal(err)
	}
	a := &expiredRowRefusingAuditor{inner: inner}
	a.refuse.Store(true)
	return a
}

func scheduleEnabled(t *testing.T, pool *pgxpool.Pool, id string) bool {
	t.Helper()
	var enabled bool
	if err := pool.QueryRow(context.Background(),
		`SELECT enabled FROM cron_schedules WHERE id = $1`, id).Scan(&enabled); err != nil {
		t.Fatalf("read schedule %s: %v", id, err)
	}
	return enabled
}

// assertOneExpiryRow checks that a schedule has exactly one audit row, and that it is the sweeper's expiry row of
// the given slot, carrying no content.
func assertOneExpiryRow(t *testing.T, pool *pgxpool.Pool, scheduleID, orgID, slot string) {
	t.Helper()
	if n := countRows(t, pool, `SELECT count(*) FROM registry_audit_log WHERE entity_id = $1`, scheduleID); n != 1 {
		t.Fatalf("%s has %d audit rows, want exactly one expiry row", scheduleID, n)
	}
	var id, kind, action, actor, org, diff string
	if err := pool.QueryRow(context.Background(), `
		SELECT id, entity_kind, action, actor, org_id, diff_json::text
		FROM registry_audit_log WHERE entity_id = $1`, scheduleID).Scan(&id, &kind, &action, &actor, &org, &diff); err != nil {
		t.Fatalf("read audit row of %s: %v", scheduleID, err)
	}
	if want := ExpiredAuditID(scheduleID, slot); id != want {
		t.Fatalf("expiry row id = %q, want ExpiredAuditID of the paused slot %q", id, want)
	}
	if kind != ContractAuditEntityKind || action != ContractAuditExpired || actor != SweeperAuditActor || org != orgID {
		t.Fatalf("expiry row = (%s, %s, %s, %s), want the sweeper's schedule expiry row for %s", kind, action, actor, org, orgID)
	}
	for _, leak := range []string{"web_search", "summarise", "approval_contract"} {
		if strings.Contains(diff, leak) {
			t.Fatalf("expiry row diff leaks %q: %s", leak, diff)
		}
	}
}

func TestExpiryAuditRowsSurviveRestartAndFailedCommit(t *testing.T) {
	pool := setupExpiryAuditDB(t)

	// A pause commits, but its expiry row is refused and waits in the process's memory. The process then stops, so
	// the queue is lost, and the sweep no longer selects the paused schedule. A fresh sweeper's durable recovery
	// recomputes the row from the schedule's next_fire_at and writes it once.
	t.Run("restart recovery", func(t *testing.T) {
		resetExpiryAuditDB(t, pool)
		ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
		defer cancel()
		insertLapsedSchedule(t, pool, "sched-restart", "org-restart")
		slot := slotOf(t, pool, "sched-restart")

		refusing := newExpiredRowRefusingAuditor(t, pool)
		before := NewSweeper(pool, allowEveryFire{}).WithContractGuards(true).WithContractAuditor(refusing)
		if fired, err := before.RunOnce(ctx); fired != 0 || err != nil {
			t.Fatalf("pausing sweep: fired=%d err=%v, want a clean pause", fired, err)
		}
		if scheduleEnabled(t, pool, "sched-restart") {
			t.Fatal("the lapsed schedule must be paused")
		}
		if n := countRows(t, pool, `SELECT count(*) FROM cron_fires WHERE schedule_id = 'sched-restart' AND status = 'paused' AND outcome = 'paused_expired' AND fire_key = $1`, slot); n != 1 {
			t.Fatalf("paused_expired fire rows at the slot = %d, want 1", n)
		}
		if refusing.refused.Load() != 1 {
			t.Fatalf("refused expiry writes = %d, want the pause's one", refusing.refused.Load())
		}
		if n := countRows(t, pool, `SELECT count(*) FROM registry_audit_log`); n != 0 {
			t.Fatalf("audit rows after the refused write = %d, want none", n)
		}
		if got := before.auditRetry.Pending(); got != 1 {
			t.Fatalf("rows queued in the stopping process = %d, want the refused expiry row", got)
		}
		if got := slotOf(t, pool, "sched-restart"); got != slot {
			t.Fatalf("the pause moved next_fire_at from %s to %s; the recovery recomputes the row from it", slot, got)
		}

		// The restart: a fresh sweeper with an empty queue and the real writer.
		auditor, err := NewContractAuditor(pool)
		if err != nil {
			t.Fatal(err)
		}
		after := NewSweeper(pool, allowEveryFire{}).WithContractGuards(true).WithContractAuditor(auditor)
		if got := after.auditRetry.Pending(); got != 0 {
			t.Fatalf("a fresh sweeper starts with %d queued rows, want none", got)
		}
		if fired, err := after.RunOnce(ctx); fired != 0 || err != nil {
			t.Fatalf("first sweep after the restart: fired=%d err=%v", fired, err)
		}
		assertOneExpiryRow(t, pool, "sched-restart", "org-restart", slot)

		// A further sweep adds none, and neither does a recovery that runs again over the same schedule.
		if _, err := after.RunOnce(ctx); err != nil {
			t.Fatalf("second sweep after the restart: %v", err)
		}
		assertOneExpiryRow(t, pool, "sched-restart", "org-restart", slot)
		again := NewSweeper(pool, allowEveryFire{}).WithContractGuards(true).WithContractAuditor(auditor)
		if _, err := again.RunOnce(ctx); err != nil {
			t.Fatalf("sweep of another restart: %v", err)
		}
		assertOneExpiryRow(t, pool, "sched-restart", "org-restart", slot)
	})

	// A pause whose expiry write fails sits in a batch that then fails at COMMIT. The pause rolls back, so its row is
	// never queued and never written. Once the commit can succeed, the next sweep pauses the schedule again and
	// writes exactly one row.
	t.Run("queued only after the batch commits", func(t *testing.T) {
		resetExpiryAuditDB(t, pool)
		ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
		defer cancel()
		insertLapsedSchedule(t, pool, "sched-uncommitted", "org-uncommitted")
		slot := slotOf(t, pool, "sched-uncommitted")
		if _, err := pool.Exec(ctx, refuseExpiredPauseAtCommit); err != nil {
			t.Fatalf("create the commit-refusing trigger: %v", err)
		}

		refusing := newExpiredRowRefusingAuditor(t, pool)
		s := NewSweeper(pool, allowEveryFire{}).WithContractGuards(true).WithContractAuditor(refusing)
		if _, err := s.RunOnce(ctx); err == nil || !strings.Contains(err.Error(), "commit refused for a paused_expired fire") {
			t.Fatalf("a sweep whose batch fails at COMMIT: err=%v, want the commit's refusal", err)
		}
		if refusing.refused.Load() != 1 {
			t.Fatalf("refused expiry writes = %d, want the pause's one", refusing.refused.Load())
		}
		if !scheduleEnabled(t, pool, "sched-uncommitted") {
			t.Fatal("the pause of a batch that did not commit must be rolled back")
		}
		if n := countRows(t, pool, `SELECT count(*) FROM cron_fires WHERE schedule_id = 'sched-uncommitted'`); n != 0 {
			t.Fatalf("fire rows of the rolled-back pause = %d, want none", n)
		}
		if n := countRows(t, pool, `SELECT count(*) FROM registry_audit_log`); n != 0 {
			t.Fatalf("audit rows after the failed commit = %d, want none", n)
		}
		if got := s.auditRetry.Pending(); got != 0 {
			t.Fatalf("rows queued for a pause that never committed = %d, want none", got)
		}

		if _, err := pool.Exec(ctx, `DROP TRIGGER cron_fires_refuse_expired_pause ON cron_fires`); err != nil {
			t.Fatalf("drop the commit-refusing trigger: %v", err)
		}
		refusing.refuse.Store(false)
		if fired, err := s.RunOnce(ctx); fired != 0 || err != nil {
			t.Fatalf("sweep once the commit can succeed: fired=%d err=%v, want a clean pause", fired, err)
		}
		if scheduleEnabled(t, pool, "sched-uncommitted") {
			t.Fatal("the lapsed schedule must be paused once its batch can commit")
		}
		assertOneExpiryRow(t, pool, "sched-uncommitted", "org-uncommitted", slot)
		if got := s.auditRetry.Pending(); got != 0 {
			t.Fatalf("rows still queued = %d, want none", got)
		}
	})
}

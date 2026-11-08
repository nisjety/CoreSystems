//go:build integration

package cron

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// Run now's own SQL is checked against a real Postgres here: the schedule read under its row
// lock, the request-id lookup, the in-flight and hourly counts (FILTER and LIKE), and the failed
// fire row a refused authorization leaves. The decisions themselves are unit-tested in
// run_now_test.go against a fake transaction; this test shows the statements agree with them.
//
// Run with: go test -tags integration -run TestRunNowDB ./internal/cron/

func runNowDBRequest(scheduleID, actor, requestID string) RunNowRequest {
	return RunNowRequest{OrgID: "org-1", ScheduleID: scheduleID, ActorID: actor, RequestID: requestID}
}

func insertManualFire(t *testing.T, pool *pgxpool.Pool, id, scheduleID, key, status string, firedAt time.Time) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), `
		INSERT INTO cron_fires (id, schedule_id, fired_at, status, fire_key)
		VALUES ($1, $2, $3, $4, $5)
	`, id, scheduleID, firedAt, status, key); err != nil {
		t.Fatalf("insert manual fire %s: %v", key, err)
	}
}

func TestRunNowDB(t *testing.T) {
	pool := setupSweepFailureDB(t)
	ctx := context.Background()
	now := time.Now().UTC().Truncate(time.Microsecond)

	t.Run("creates one pending manual fire and leaves the cron slot alone", func(t *testing.T) {
		insertDueSchedule(t, pool, "run-now-slot", "org-1", "Daglig rapport", -time.Hour)
		var nextBefore time.Time
		if err := pool.QueryRow(ctx, `SELECT next_fire_at FROM cron_schedules WHERE id = $1`, "run-now-slot").Scan(&nextBefore); err != nil {
			t.Fatalf("read slot: %v", err)
		}
		authorizer := &recordingRunNowAuthorizer{}

		result, refusal, err := RunNow(ctx, pool, authorizer, runNowDBRequest("run-now-slot", "user-1", "req-slot-0000000001"), now)
		if err != nil || refusal != nil {
			t.Fatalf("RunNow = %+v, %+v, %v; want a pending fire", result, refusal, err)
		}
		if result.Status != "pending" || result.FireKey != "manual:req-slot-0000000001" || result.TaskID == "" || result.Deduplicated {
			t.Fatalf("result = %+v", result)
		}
		if authorizer.calls != 1 || authorizer.intent.FireKey != result.FireKey || authorizer.intent.SubjectID != "user-1" {
			t.Fatalf("authorizer = %d calls, intent %+v", authorizer.calls, authorizer.intent)
		}
		assertFireRow(t, pool, "run-now-slot", "pending", "manual:req-slot-0000000001", "")
		if n := countRows(t, pool, `SELECT count(*) FROM tasks WHERE id = $1 AND org_id = 'org-1'`, result.TaskID); n != 1 {
			t.Fatalf("tasks row for %s: %d, want 1", result.TaskID, n)
		}

		var nextAfter time.Time
		var lastFire *time.Time
		if err := pool.QueryRow(ctx, `SELECT next_fire_at, last_fire_at FROM cron_schedules WHERE id = $1`, "run-now-slot").Scan(&nextAfter, &lastFire); err != nil {
			t.Fatalf("read schedule: %v", err)
		}
		if !nextAfter.Equal(nextBefore) || lastFire != nil {
			t.Fatalf("run now moved the slot: next %v -> %v, last_fire_at %v", nextBefore, nextAfter, lastFire)
		}
	})

	t.Run("answers a retried request with the fire it made first", func(t *testing.T) {
		insertDueSchedule(t, pool, "run-now-retry", "org-1", "Retry", -time.Hour)
		authorizer := &recordingRunNowAuthorizer{}
		first, refusal, err := RunNow(ctx, pool, authorizer, runNowDBRequest("run-now-retry", "user-1", "req-retry-000000001"), now)
		if err != nil || refusal != nil {
			t.Fatalf("first press = %+v, %+v, %v", first, refusal, err)
		}

		second, refusal, err := RunNow(ctx, pool, authorizer, runNowDBRequest("run-now-retry", "user-1", "req-retry-000000001"), now.Add(time.Second))
		if err != nil || refusal != nil {
			t.Fatalf("retry = %+v, %+v, %v", second, refusal, err)
		}
		if !second.Deduplicated || second.FireID != first.FireID || second.TaskID != first.TaskID {
			t.Fatalf("retry = %+v, want the first fire %+v", second, first)
		}
		if authorizer.calls != 1 {
			t.Fatalf("authorizer called %d times, want 1 (a retry asks Control nothing new)", authorizer.calls)
		}
		if n := countRows(t, pool, `SELECT count(*) FROM cron_fires WHERE schedule_id = $1`, "run-now-retry"); n != 1 {
			t.Fatalf("retry wrote %d fire rows, want 1", n)
		}
	})

	t.Run("refuses a second press while the first manual fire is still pending", func(t *testing.T) {
		insertDueSchedule(t, pool, "run-now-busy", "org-1", "Busy", -time.Hour)
		authorizer := &recordingRunNowAuthorizer{}
		if _, refusal, err := RunNow(ctx, pool, authorizer, runNowDBRequest("run-now-busy", "user-1", "req-busy-0000000001"), now); err != nil || refusal != nil {
			t.Fatalf("first press refused: %+v, %v", refusal, err)
		}

		_, refusal, err := RunNow(ctx, pool, authorizer, runNowDBRequest("run-now-busy", "user-1", "req-busy-0000000002"), now.Add(time.Minute))
		wantRefusal(t, refusal, err, http.StatusConflict, "run_now_in_flight")
		if n := countRows(t, pool, `SELECT count(*) FROM cron_fires WHERE schedule_id = $1`, "run-now-busy"); n != 1 {
			t.Fatalf("refused press wrote rows: %d fire rows, want 1", n)
		}
	})

	t.Run("counts the manual fires of the last hour, and only those", func(t *testing.T) {
		insertDueSchedule(t, pool, "run-now-limit", "org-1", "Limit", -time.Hour)
		for i := 0; i < 6; i++ {
			key := fmt.Sprintf("manual:old-%02d", i)
			insertManualFire(t, pool, "cronfire_old_"+key, "run-now-limit", key, "completed", now.Add(-2*time.Hour))
		}
		authorizer := &recordingRunNowAuthorizer{}
		first, refusal, err := RunNow(ctx, pool, authorizer, runNowDBRequest("run-now-limit", "user-1", "req-limit-0000000001"), now)
		if err != nil || refusal != nil {
			t.Fatalf("fires older than an hour blocked a press: %+v, %v", refusal, err)
		}
		if _, err := pool.Exec(ctx, `UPDATE cron_fires SET status = 'completed' WHERE fire_key = $1`, first.FireKey); err != nil {
			t.Fatalf("finish first fire: %v", err)
		}
		for i := 0; i < 5; i++ {
			key := fmt.Sprintf("manual:recent-%02d", i)
			insertManualFire(t, pool, "cronfire_recent_"+key, "run-now-limit", key, "completed", now.Add(-10*time.Minute))
		}

		before := countRows(t, pool, `SELECT count(*) FROM cron_fires WHERE schedule_id = $1`, "run-now-limit")
		_, refusal, err = RunNow(ctx, pool, authorizer, runNowDBRequest("run-now-limit", "user-1", "req-limit-0000000002"), now)
		wantRefusal(t, refusal, err, http.StatusTooManyRequests, "run_now_rate_limited")
		if after := countRows(t, pool, `SELECT count(*) FROM cron_fires WHERE schedule_id = $1`, "run-now-limit"); after != before {
			t.Fatalf("refused press wrote rows: %d -> %d", before, after)
		}
	})

	t.Run("records a failed fire when Control refuses, and keeps the schedule enabled", func(t *testing.T) {
		insertDueSchedule(t, pool, "run-now-denied", "org-1", "Denied", -time.Hour)
		authorizer := &recordingRunNowAuthorizer{err: errors.New("control refused")}

		_, refusal, err := RunNow(ctx, pool, authorizer, runNowDBRequest("run-now-denied", "user-1", "req-denied-0000000001"), now)
		wantRefusal(t, refusal, err, http.StatusForbidden, "fire_not_authorized")
		assertFireRow(t, pool, "run-now-denied", "failed", "manual:req-denied-0000000001", "fresh schedule authorization failed")

		var enabled bool
		if err := pool.QueryRow(ctx, `SELECT enabled FROM cron_schedules WHERE id = $1`, "run-now-denied").Scan(&enabled); err != nil {
			t.Fatalf("read schedule: %v", err)
		}
		if !enabled {
			t.Fatal("a refused manual fire disabled the schedule; only the sweeper may do that")
		}
	})

	t.Run("refuses another member and writes nothing", func(t *testing.T) {
		insertDueSchedule(t, pool, "run-now-other", "org-1", "Other", -time.Hour)
		authorizer := &recordingRunNowAuthorizer{}

		_, refusal, err := RunNow(ctx, pool, authorizer, runNowDBRequest("run-now-other", "user-2", "req-other-0000000001"), now)
		wantRefusal(t, refusal, err, http.StatusForbidden, "run_now_not_creator")
		if authorizer.calls != 0 {
			t.Fatalf("authorizer asked for a stranger's schedule: %d calls", authorizer.calls)
		}
		if n := countRows(t, pool, `SELECT count(*) FROM cron_fires WHERE schedule_id = $1`, "run-now-other"); n != 0 {
			t.Fatalf("refused press wrote %d fire rows", n)
		}
	})
}

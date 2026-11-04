package cron

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// runNowTx stands in for the one transaction a manual fire is made in. It answers the
// schedule read, the request-id lookup and the manual-fire counts from its fields, and
// records every write and whether the transaction committed.
type runNowTx struct {
	pgx.Tx
	// template is the stored task template; nil means the schedule does not exist.
	template  json.RawMessage
	creator   string
	expr      string
	existing  *RunNowResult
	inFlight  int
	lastHour  int
	execs     []recordedExec
	committed bool
}

func (tx *runNowTx) Begin(context.Context) (pgx.Tx, error) { return tx, nil }

func (tx *runNowTx) Commit(context.Context) error {
	tx.committed = true
	return nil
}

func (tx *runNowTx) Rollback(context.Context) error { return nil }

func (tx *runNowTx) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	tx.execs = append(tx.execs, recordedExec{sql: sql, args: args})
	return pgconn.CommandTag{}, nil
}

func (tx *runNowTx) QueryRow(_ context.Context, sql string, _ ...any) pgx.Row {
	switch {
	case strings.Contains(sql, "FROM cron_schedules"):
		return scanFunc(func(dest ...any) error {
			if tx.template == nil {
				return pgx.ErrNoRows
			}
			*dest[0].(*string) = "sched-1"
			*dest[1].(*string) = "org-1"
			*dest[2].(*string) = "space-1"
			*dest[3].(*string) = tx.creator
			*dest[4].(*string) = "sha256:" + strings.Repeat("a", 64)
			*dest[5].(*string) = tx.expr
			*dest[6].(*string) = "UTC"
			*dest[7].(*json.RawMessage) = tx.template
			return nil
		})
	case strings.Contains(sql, "count(*)"):
		return scanFunc(func(dest ...any) error {
			*dest[0].(*int) = tx.inFlight
			*dest[1].(*int) = tx.lastHour
			return nil
		})
	default:
		return scanFunc(func(dest ...any) error {
			if tx.existing == nil {
				return pgx.ErrNoRows
			}
			*dest[0].(*string) = tx.existing.FireID
			*dest[1].(*string) = tx.existing.TaskID
			*dest[2].(*string) = tx.existing.Status
			return nil
		})
	}
}

type scanFunc func(dest ...any) error

func (f scanFunc) Scan(dest ...any) error { return f(dest...) }

// recordingRunNowAuthorizer keeps the intent it was asked to authorise and answers err.
type recordingRunNowAuthorizer struct {
	intent FireIntent
	calls  int
	err    error
}

func (a *recordingRunNowAuthorizer) AuthorizeFire(_ context.Context, intent FireIntent) error {
	a.calls++
	a.intent = intent
	return a.err
}

const runNowRequestIDForTest = "6f1c2a9e-0b7d-4c55-9a1e-3d2f8b7c6a10"

func runNowNow() time.Time { return time.Date(2026, 11, 4, 9, 0, 0, 0, time.UTC) }

// liveRunNowTx is a schedule of user-1 with a live approval and nothing fired yet.
func liveRunNowTx(t *testing.T) *runNowTx {
	return &runNowTx{template: guardTemplate(t, runNowNow().Add(30*24*time.Hour)), creator: "user-1", expr: "0 9 * * *"}
}

func runNowAs(actor string) RunNowRequest {
	return RunNowRequest{OrgID: "org-1", ScheduleID: "sched-1", ActorID: actor, RequestID: runNowRequestIDForTest}
}

func wantRefusal(t *testing.T, refusal *RunNowRefusal, err error, status int, code string) {
	t.Helper()
	if err != nil {
		t.Fatalf("RunNow error: %v", err)
	}
	if refusal == nil || refusal.HTTPStatus != status || refusal.Code != code {
		t.Fatalf("refusal = %+v, want %d %s", refusal, status, code)
	}
}

func TestRunNowCreatesPendingFireWithManualKey(t *testing.T) {
	tx := liveRunNowTx(t)
	authorizer := &recordingRunNowAuthorizer{}
	result, refusal, err := RunNow(context.Background(), tx, authorizer, runNowAs("user-1"), runNowNow())
	if err != nil || refusal != nil {
		t.Fatalf("RunNow: refusal=%+v err=%v", refusal, err)
	}
	wantKey := "manual:" + runNowRequestIDForTest
	if result.FireKey != wantKey || result.Status != "pending" || result.Deduplicated ||
		!strings.HasPrefix(result.FireID, "cronfire_") || !strings.HasPrefix(result.TaskID, "task_") {
		t.Fatalf("result = %+v", result)
	}
	if authorizer.calls != 1 || authorizer.intent.FireKey != wantKey || authorizer.intent.SubjectID != "user-1" ||
		authorizer.intent.IdempotencyKey != "sched-1:"+wantKey || authorizer.intent.ScheduleID != "sched-1" {
		t.Fatalf("Control was asked for %+v (%d calls)", authorizer.intent, authorizer.calls)
	}
	if len(tx.execs) != 2 || !strings.Contains(tx.execs[0].sql, "INSERT INTO tasks") ||
		!strings.Contains(tx.execs[1].sql, "'pending'") {
		t.Fatalf("writes = %+v, want one task and one pending fire", tx.execs)
	}
	if got := tx.execs[1].args[4]; got != wantKey {
		t.Fatalf("fire row key = %v, want %s", got, wantKey)
	}
	var config struct {
		Intent FireIntent `json:"schedule_fire_intent"`
	}
	if err := json.Unmarshal(tx.execs[0].args[7].([]byte), &config); err != nil {
		t.Fatal(err)
	}
	if config.Intent.FireKey != wantKey || config.Intent.SubjectID != "user-1" {
		t.Fatalf("task carries intent %+v, want the manual key under the creator", config.Intent)
	}
	if !tx.committed {
		t.Fatal("the manual fire was not committed")
	}
}

func TestRunNowRefusesNonCreator(t *testing.T) {
	for _, actor := range []string{"user-2", ""} {
		tx := liveRunNowTx(t)
		authorizer := &recordingRunNowAuthorizer{}
		_, refusal, err := RunNow(context.Background(), tx, authorizer, runNowAs(actor), runNowNow())
		wantRefusal(t, refusal, err, http.StatusForbidden, "run_now_not_creator")
		if authorizer.calls != 0 || len(tx.execs) != 0 || tx.committed {
			t.Fatalf("actor %q: a refused press reached Control or wrote: calls=%d writes=%+v", actor, authorizer.calls, tx.execs)
		}
	}
}

func TestRunNowRefusesUnknownSchedule(t *testing.T) {
	tx := &runNowTx{}
	_, refusal, err := RunNow(context.Background(), tx, &recordingRunNowAuthorizer{}, runNowAs("user-1"), runNowNow())
	wantRefusal(t, refusal, err, http.StatusNotFound, "not_found")
}

func TestRunNowRefusesBadRequestIDBeforeTheStore(t *testing.T) {
	for _, id := range []string{"", "short", "has/slash-0123456789", strings.Repeat("a", 65)} {
		req := runNowAs("user-1")
		req.RequestID = id
		_, refusal, err := RunNow(context.Background(), nil, &recordingRunNowAuthorizer{}, req, runNowNow())
		wantRefusal(t, refusal, err, http.StatusBadRequest, "invalid_request_id")
	}
}

func TestRunNowRefusesLapsedApproval(t *testing.T) {
	tx := liveRunNowTx(t)
	tx.template = guardTemplate(t, runNowNow().Add(-time.Minute))
	authorizer := &recordingRunNowAuthorizer{}
	_, refusal, err := RunNow(context.Background(), tx, authorizer, runNowAs("user-1"), runNowNow())
	wantRefusal(t, refusal, err, http.StatusConflict, "approval_expired")
	if authorizer.calls != 0 || len(tx.execs) != 0 {
		t.Fatalf("a lapsed approval reached Control or wrote: calls=%d writes=%+v", authorizer.calls, tx.execs)
	}
}

func TestRunNowRefusesInvalidExpression(t *testing.T) {
	tx := liveRunNowTx(t)
	tx.expr = "not a cron expression"
	_, refusal, err := RunNow(context.Background(), tx, &recordingRunNowAuthorizer{}, runNowAs("user-1"), runNowNow())
	wantRefusal(t, refusal, err, http.StatusConflict, "invalid_schedule")
}

func TestRunNowDeduplicatesSameRequestID(t *testing.T) {
	tx := liveRunNowTx(t)
	tx.existing = &RunNowResult{FireID: "cronfire_first", TaskID: "task_first", Status: "running"}
	authorizer := &recordingRunNowAuthorizer{}
	result, refusal, err := RunNow(context.Background(), tx, authorizer, runNowAs("user-1"), runNowNow())
	if err != nil || refusal != nil {
		t.Fatalf("RunNow: refusal=%+v err=%v", refusal, err)
	}
	if !result.Deduplicated || result.FireID != "cronfire_first" || result.TaskID != "task_first" ||
		result.Status != "running" || result.FireKey != "manual:"+runNowRequestIDForTest {
		t.Fatalf("result = %+v, want the first fire back", result)
	}
	if authorizer.calls != 0 || len(tx.execs) != 0 {
		t.Fatalf("a retried request fired again: calls=%d writes=%+v", authorizer.calls, tx.execs)
	}
}

func TestRunNowRefusesWhileManualFireInFlight(t *testing.T) {
	tx := liveRunNowTx(t)
	tx.inFlight = 1
	authorizer := &recordingRunNowAuthorizer{}
	_, refusal, err := RunNow(context.Background(), tx, authorizer, runNowAs("user-1"), runNowNow())
	wantRefusal(t, refusal, err, http.StatusConflict, "run_now_in_flight")
	if authorizer.calls != 0 || len(tx.execs) != 0 {
		t.Fatalf("an in-flight refusal reached Control or wrote: calls=%d writes=%+v", authorizer.calls, tx.execs)
	}
}

func TestRunNowRateLimit(t *testing.T) {
	tx := liveRunNowTx(t)
	tx.lastHour = runNowHourlyLimit
	_, refusal, err := RunNow(context.Background(), tx, &recordingRunNowAuthorizer{}, runNowAs("user-1"), runNowNow())
	wantRefusal(t, refusal, err, http.StatusTooManyRequests, "run_now_rate_limited")

	// One under the limit still fires.
	tx = liveRunNowTx(t)
	tx.lastHour = runNowHourlyLimit - 1
	if _, refusal, err := RunNow(context.Background(), tx, &recordingRunNowAuthorizer{}, runNowAs("user-1"), runNowNow()); err != nil || refusal != nil {
		t.Fatalf("fire under the limit: refusal=%+v err=%v", refusal, err)
	}
}

func TestRunNowAuthorizerRefusalWritesFailedFireAndKeepsScheduleEnabled(t *testing.T) {
	tx := liveRunNowTx(t)
	authorizer := &recordingRunNowAuthorizer{err: errors.New("Control refused")}
	_, refusal, err := RunNow(context.Background(), tx, authorizer, runNowAs("user-1"), runNowNow())
	wantRefusal(t, refusal, err, http.StatusForbidden, "fire_not_authorized")
	if len(tx.execs) != 1 {
		t.Fatalf("writes = %+v, want one failed fire", tx.execs)
	}
	write := tx.execs[0]
	if !strings.Contains(write.sql, "INSERT INTO cron_fires") || !strings.Contains(write.sql, "'failed'") ||
		write.args[4] != "manual:"+runNowRequestIDForTest {
		t.Fatalf("refusal wrote %+v, want a failed fire under the manual key", write)
	}
	for _, exec := range tx.execs {
		if strings.Contains(exec.sql, "enabled=false") || strings.Contains(exec.sql, "INSERT INTO tasks") {
			t.Fatalf("a refused manual fire disabled the schedule or made a task: %s", exec.sql)
		}
	}
	if !tx.committed {
		t.Fatal("the failed fire was not committed, so the refusal leaves no record")
	}
}

func TestRunNowDoesNotMoveNextFireAt(t *testing.T) {
	tx := liveRunNowTx(t)
	if _, refusal, err := RunNow(context.Background(), tx, &recordingRunNowAuthorizer{}, runNowAs("user-1"), runNowNow()); err != nil || refusal != nil {
		t.Fatalf("RunNow: refusal=%+v err=%v", refusal, err)
	}
	for _, exec := range tx.execs {
		if strings.Contains(exec.sql, "cron_schedules") || strings.Contains(exec.sql, "next_fire_at") ||
			strings.Contains(exec.sql, "last_fire_at") {
			t.Fatalf("Run now wrote the schedule's slot bookkeeping: %s", exec.sql)
		}
	}
}

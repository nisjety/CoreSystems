package rawstore

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// R6: migration 0005 refuses an evidence row unless its run_id names a run that is
// running. The refusal is SQLSTATE 55000 and its message names the table and the run,
// never row data. The store maps it to an error that wraps ErrStaleWrite.

const sqlNotInPrerequisiteState = "55000" // object_not_in_prerequisite_state

// evidenceKind is one evidence table and the two ways a test writes a row into it:
// through the store, and by hand with SQL. n makes the row unique within a database.
type evidenceKind struct {
	table    string
	viaStore func(ctx context.Context, s *Store, run int64, n int) error
	bySQL    func(ctx context.Context, pool *pgxpool.Pool, run int64, n int) error
}

func hexSHA(b []byte) string {
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func guardID(n int) string { return fmt.Sprintf("2026-%06d", 300000+n) }

var evidenceKinds = []evidenceKind{
	{
		table: "raw_documents",
		viaStore: func(ctx context.Context, s *Store, run int64, n int) error {
			_, err := s.StoreNotice(ctx, run, guardNotice(guardID(n), []byte("body "+strconv.Itoa(n))))
			return err
		},
		bySQL: func(ctx context.Context, pool *pgxpool.Pool, run int64, n int) error {
			body := []byte("body " + strconv.Itoa(n))
			_, err := pool.Exec(ctx, `
				INSERT INTO raw_documents (source, doffin_id, content_type, body, body_sha256, fetched_at, run_id)
				VALUES ('doffin', $1, '', $2, $3, now(), $4)`, guardID(n), body, hexSHA(body), run)
			return err
		},
	},
	{
		table: "raw_search_pages",
		viaStore: func(ctx context.Context, s *Store, run int64, n int) error {
			return s.PutSearchPage(ctx, run, doffinsync.SearchPage{
				QueryKey: "guard-" + strconv.Itoa(n), Page: 1, ContentType: "application/json",
				Body: []byte("page " + strconv.Itoa(n)), FetchedAt: fixedNow,
			})
		},
		bySQL: func(ctx context.Context, pool *pgxpool.Pool, run int64, n int) error {
			body := []byte("page " + strconv.Itoa(n))
			_, err := pool.Exec(ctx, `
				INSERT INTO raw_search_pages (source, query_key, page, content_type, body, body_sha256, fetched_at, run_id)
				VALUES ('doffin', $1, 1, '', $2, $3, now(), $4)`, "guard-"+strconv.Itoa(n), body, hexSHA(body), run)
			return err
		},
	},
	{
		table: "download_attempts",
		viaStore: func(ctx context.Context, s *Store, run int64, n int) error {
			return s.RecordAttempt(ctx, run, doffinsync.Attempt{
				DoffinID: guardID(n), Outcome: doffinsync.OutcomeHTTPError, HTTPStatus: 503,
				Message: "busy", At: fixedNow,
			})
		},
		bySQL: func(ctx context.Context, pool *pgxpool.Pool, run int64, n int) error {
			_, err := pool.Exec(ctx, `
				INSERT INTO download_attempts (run_id, source, doffin_id, attempted_at, outcome, http_status, message)
				VALUES ($1, 'doffin', $2, now(), 'http_error', 503, 'busy')`, run, guardID(n))
			return err
		},
	},
}

// runsInEveryState opens one run per state a run can be in: running, interrupted by
// recovery, and each reason FinishRun can end a run with. Recovery interrupts every
// run that is running, so the interrupted run is made first and the running one last.
func runsInEveryState(t *testing.T, s *Store) map[string]int64 {
	t.Helper()
	ctx := testCtx(t)
	runs := make(map[string]int64)

	runs["interrupted"] = beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	if _, err := s.RecoverStale(ctx); err != nil {
		t.Fatalf("RecoverStale: %v", err)
	}
	for _, reason := range []doffinsync.Reason{
		doffinsync.ReasonDone, doffinsync.ReasonPartial, doffinsync.ReasonPaused,
		doffinsync.ReasonKeyRejected, doffinsync.ReasonCancelled, doffinsync.ReasonFailed,
	} {
		run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
		if err := s.FinishRun(ctx, run, doffinsync.RunOutcome{Reason: reason}); err != nil {
			t.Fatalf("FinishRun %s: %v", reason, err)
		}
		runs[string(reason)] = run
	}
	runs["running"] = beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	return runs
}

// assertRefusedByGuard fails unless err is the run guard's refusal: SQLSTATE 55000,
// and for a write through the store an error that also wraps ErrStaleWrite.
func assertRefusedByGuard(t *testing.T, what string, err error, viaStore bool) {
	t.Helper()
	switch {
	case err == nil:
		t.Errorf("%s: the insert succeeded, want it refused", what)
	case pgCode(err) != sqlNotInPrerequisiteState:
		t.Errorf("%s: SQLSTATE %q (%v), want 55000", what, pgCode(err), err)
	case viaStore && !errors.Is(err, ErrStaleWrite):
		t.Errorf("%s: err = %v, want it to wrap ErrStaleWrite", what, err)
	}
}

// R6: every state but running refuses an evidence insert, on each of the three
// evidence tables, whether the row comes through the store or by hand. Nothing is
// written. The store's error wraps ErrStaleWrite and keeps the database error.
func TestEvidenceRunGuardRefusesEveryRunThatIsNotRunning(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	runs := runsInEveryState(t, s)

	for _, kind := range evidenceKinds {
		t.Run(kind.table, func(t *testing.T) {
			n := 0
			for state, run := range runs {
				if state == "running" {
					continue
				}
				n++
				assertRefusedByGuard(t, state+" via the store", kind.viaStore(ctx, s, run, n), true)
				n++
				assertRefusedByGuard(t, state+" by SQL", kind.bySQL(ctx, pool, run, n), false)
			}
			if got := count(t, pool, `SELECT count(*) FROM `+kind.table); got != 0 {
				t.Errorf("%s holds %d row(s) after the refused inserts, want none", kind.table, got)
			}
		})
	}
}

// R6: a run that is running takes evidence on each of the three tables, through the
// store and by hand. The control that shows the refusals above are about the run's
// state and not about the statements.
func TestEvidenceRunGuardAllowsARunningRun(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	for _, kind := range evidenceKinds {
		if err := kind.viaStore(ctx, s, run, 1); err != nil {
			t.Errorf("%s via the store under a running run: %v", kind.table, err)
		}
		if err := kind.bySQL(ctx, pool, run, 2); err != nil {
			t.Errorf("%s by SQL under a running run: %v", kind.table, err)
		}
		if got := count(t, pool, `SELECT count(*) FROM `+kind.table); got < 2 {
			t.Errorf("%s holds %d row(s), want the two just written", kind.table, got)
		}
	}
}

// R6: a run id that names no run is refused by the guard too, before the foreign key
// would have refused it with another code.
func TestEvidenceRunGuardRefusesAnUnknownRun(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	const missing = 424242

	for _, kind := range evidenceKinds {
		assertRefusedByGuard(t, kind.table+" via the store", kind.viaStore(ctx, s, missing, 1), true)
		assertRefusedByGuard(t, kind.table+" by SQL", kind.bySQL(ctx, pool, missing, 2), false)
	}
}

// R6: the refusal names the table and the run, and carries no row data: not the
// notice id, not the body, not the query key and not the attempt message.
func TestEvidenceRunGuardMessageNamesTableAndRunNeverRowData(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	if err := s.FinishRun(ctx, run, doffinsync.RunOutcome{Reason: doffinsync.ReasonDone}); err != nil {
		t.Fatalf("FinishRun: %v", err)
	}

	const (
		marker = "ROWDATA-MARKER-9f2c"
		rowID  = "2026-777777"
	)
	body := []byte("body " + marker)
	inserts := []struct {
		table string
		err   error
	}{
		{"raw_documents", func() error {
			_, err := pool.Exec(ctx, `
				INSERT INTO raw_documents (source, doffin_id, content_type, body, body_sha256, fetched_at, run_id)
				VALUES ('doffin', $1, $2, $3, $4, now(), $5)`, rowID, "text/"+marker, body, hexSHA(body), run)
			return err
		}()},
		{"raw_search_pages", func() error {
			_, err := pool.Exec(ctx, `
				INSERT INTO raw_search_pages (source, query_key, page, content_type, body, body_sha256, fetched_at, run_id)
				VALUES ('doffin', $1, 1, $2, $3, $4, now(), $5)`, "key-"+marker, "text/"+marker, body, hexSHA(body), run)
			return err
		}()},
		{"download_attempts", func() error {
			_, err := pool.Exec(ctx, `
				INSERT INTO download_attempts (run_id, source, doffin_id, attempted_at, outcome, http_status, message)
				VALUES ($1, 'doffin', $2, now(), 'http_error', 503, $3)`, run, rowID, marker)
			return err
		}()},
	}
	for _, in := range inserts {
		assertRefusedByGuard(t, in.table, in.err, false)
		if in.err == nil {
			continue
		}
		msg := in.err.Error()
		for _, want := range []string{in.table, "run " + strconv.FormatInt(run, 10)} {
			if !strings.Contains(msg, want) {
				t.Errorf("%s: refusal %q does not name %q", in.table, msg, want)
			}
		}
		for _, leak := range []string{marker, rowID, hexSHA(body)} {
			if strings.Contains(msg, leak) {
				t.Errorf("%s: refusal %q carries row data %q", in.table, msg, leak)
			}
		}
	}
}

// R6: INSERT ... ON CONFLICT DO NOTHING fires a BEFORE INSERT trigger before it looks
// for a conflict, so a row that is already held is refused too, not skipped. A stale
// process cannot even re-send a duplicate under a run that has ended.
func TestEvidenceRunGuardFiresBeforeTheConflictCheck(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	n := guardNotice("2026-100001", []byte("held body"))
	page := doffinsync.SearchPage{QueryKey: testQueryKey, Page: 1, ContentType: "application/json", Body: []byte(`{"hits":[]}`), FetchedAt: fixedNow}

	if _, err := s.StoreNotice(ctx, run, n); err != nil {
		t.Fatalf("StoreNotice under the running run: %v", err)
	}
	if err := s.PutSearchPage(ctx, run, page); err != nil {
		t.Fatalf("PutSearchPage under the running run: %v", err)
	}
	if err := s.FinishRun(ctx, run, doffinsync.RunOutcome{Reason: doffinsync.ReasonDone}); err != nil {
		t.Fatalf("FinishRun: %v", err)
	}

	_, err := s.StoreNotice(ctx, run, n)
	assertRefusedByGuard(t, "a duplicate notice after the run ended", err, true)
	assertRefusedByGuard(t, "a duplicate page after the run ended", s.PutSearchPage(ctx, run, page), true)
}

// waitForLockWaiter returns once some session of the test database waits on a lock,
// and fails the test when the call under test returns first, or nothing waits in time.
func waitForLockWaiter(t *testing.T, pool *pgxpool.Pool, returned <-chan error, what string) {
	t.Helper()
	deadline := time.Now().Add(15 * time.Second)
	for time.Now().Before(deadline) {
		select {
		case err := <-returned:
			t.Fatalf("%s returned (%v) instead of waiting for the lock", what, err)
		default:
		}
		if count(t, pool, `
			SELECT count(*) FROM pg_stat_activity
			WHERE datname = current_database() AND wait_event_type = 'Lock'`) > 0 {
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatalf("%s did not wait for the lock", what)
}

// R6: the guard checks the run's state under a share lock, so an evidence insert
// cannot slip in beside a run that is being ended. While a transaction is ending the
// run, the insert waits for it, and after it commits the insert is refused.
func TestEvidenceInsertWaitsForARunThatIsEnding(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	ending, err := pool.Begin(ctx)
	if err != nil {
		t.Fatalf("Begin: %v", err)
	}
	defer func() { _ = ending.Rollback(ctx) }()
	if _, err := ending.Exec(ctx, `UPDATE sync_runs SET state = 'failed', finished_at = now() WHERE id = $1`, run); err != nil {
		t.Fatalf("end the run inside the open transaction: %v", err)
	}

	returned := make(chan error, 1)
	go func() {
		returned <- s.RecordAttempt(ctx, run, doffinsync.Attempt{
			DoffinID: guardID(1), Outcome: doffinsync.OutcomeHTTPError, HTTPStatus: 503, Message: "busy", At: fixedNow,
		})
	}()
	waitForLockWaiter(t, pool, returned, "RecordAttempt")
	if err := ending.Commit(ctx); err != nil {
		t.Fatalf("commit the end of the run: %v", err)
	}

	select {
	case err := <-returned:
		assertRefusedByGuard(t, "an attempt that waited for the run to end", err, true)
	case <-time.After(15 * time.Second):
		t.Fatal("RecordAttempt did not return after the run ended")
	}
	if got := count(t, pool, `SELECT count(*) FROM download_attempts`); got != 0 {
		t.Fatalf("download_attempts holds %d row(s), want none", got)
	}
}

// R6: the same lock runs the other way. A run cannot end while an evidence insert is
// in flight under it, so recovery and FinishRun wait for the insert, and the evidence
// is on record before the run is marked ended.
func TestRunEndWaitsForEvidenceInFlight(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")

	inflight, err := pool.Begin(ctx)
	if err != nil {
		t.Fatalf("Begin: %v", err)
	}
	defer func() { _ = inflight.Rollback(ctx) }()
	if _, err := inflight.Exec(ctx, `
		INSERT INTO download_attempts (run_id, source, doffin_id, attempted_at, outcome, http_status, message)
		VALUES ($1, 'doffin', $2, now(), 'http_error', 503, 'busy')`, run, guardID(1)); err != nil {
		t.Fatalf("insert evidence inside the open transaction: %v", err)
	}

	returned := make(chan error, 1)
	go func() {
		returned <- s.FinishRun(ctx, run, doffinsync.RunOutcome{Reason: doffinsync.ReasonDone})
	}()
	waitForLockWaiter(t, pool, returned, "FinishRun")
	if err := inflight.Commit(ctx); err != nil {
		t.Fatalf("commit the evidence: %v", err)
	}

	select {
	case err := <-returned:
		if err != nil {
			t.Fatalf("FinishRun after the evidence committed: %v", err)
		}
	case <-time.After(15 * time.Second):
		t.Fatal("FinishRun did not return after the evidence committed")
	}
	if got := runState(t, pool, run); !strings.HasPrefix(got, "done|") {
		t.Fatalf("run = %q, want done", got)
	}
	if got := count(t, pool, `SELECT count(*) FROM download_attempts WHERE run_id = $1`, run); got != 1 {
		t.Fatalf("download_attempts under the run = %d, want the one that was in flight", got)
	}
}

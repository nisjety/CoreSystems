package rawstore

import (
	"errors"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// runState reads a run's state and end time as one string, so that a comparison
// before and after an operation catches any change to either.
func runState(t *testing.T, pool *pgxpool.Pool, run int64) string {
	t.Helper()
	var state, finished string
	if err := pool.QueryRow(testCtx(t),
		`SELECT state, COALESCE(finished_at::text, '') FROM sync_runs WHERE id = $1`, run).
		Scan(&state, &finished); err != nil {
		t.Fatalf("read run %d: %v", run, err)
	}
	return state + "|" + finished
}

// dayState reads a day's state and error code as one string.
func dayState(t *testing.T, pool *pgxpool.Pool, label string) string {
	t.Helper()
	var state, code string
	if err := pool.QueryRow(testCtx(t),
		`SELECT state, COALESCE(error_code, '') FROM sync_days WHERE source = 'doffin' AND day = $1::date`, label).
		Scan(&state, &code); err != nil {
		t.Fatalf("read day %s: %v", label, err)
	}
	return state + "|" + code
}

// S5 (review PG-02, D-B5): a run that recovery marked interrupted cannot write its
// day again, and cannot begin one. The day keeps the state recovery gave it.
func TestFinishDayFromRecoveredRunChangesNothing(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	startDay(t, s, "2026-10-01", run)
	if _, err := s.RecoverStale(ctx); err != nil {
		t.Fatalf("RecoverStale: %v", err)
	}

	err := s.FinishDay(ctx, doffinsync.DayRecord{
		Day: utcDay(t, "2026-10-01"), RunID: run, State: doffinsync.StateDone,
		Total: int64Ptr(2), Accessible: int64Ptr(2), Stored: 2,
	})
	if !errors.Is(err, ErrStaleWrite) {
		t.Fatalf("FinishDay from a recovered run: err = %v, want ErrStaleWrite", err)
	}
	if got := dayState(t, pool, "2026-10-01"); got != "interrupted|process_lost" {
		t.Fatalf("day after the refused FinishDay = %q, want interrupted|process_lost", got)
	}
	if got := count(t, pool, `SELECT count(*) FROM sync_days WHERE source = 'doffin' AND day = '2026-10-01'::date AND stored = 0 AND num_total IS NULL`); got != 1 {
		t.Fatalf("the refused FinishDay changed the day's counts")
	}

	if err := s.BeginDay(ctx, utcDay(t, "2026-10-01"), run); !errors.Is(err, ErrStaleWrite) {
		t.Fatalf("BeginDay from a recovered run: err = %v, want ErrStaleWrite", err)
	}
}

// S5 (D-B5): a finished run cannot begin a day, so the ledger gains no row for it.
func TestBeginDayOnAFinishedRunIsRefused(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	if err := s.FinishRun(ctx, run, doffinsync.RunOutcome{Reason: doffinsync.ReasonDone}); err != nil {
		t.Fatalf("FinishRun: %v", err)
	}

	if err := s.BeginDay(ctx, utcDay(t, "2026-10-01"), run); !errors.Is(err, ErrStaleWrite) {
		t.Fatalf("BeginDay on a finished run: err = %v, want ErrStaleWrite", err)
	}
	if got := count(t, pool, `SELECT count(*) FROM sync_days`); got != 0 {
		t.Fatalf("day rows after a refused BeginDay = %d, want 0", got)
	}
}

// S7 (review PG-08): recovery touches only running work. A finished run and a done
// day keep their state and their end time, and only the live run is recovered.
func TestRecoverStaleLeavesFinishedWorkAlone(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)

	finished := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	startDay(t, s, "2026-10-01", finished)
	if err := s.FinishDay(ctx, doffinsync.DayRecord{
		Day: utcDay(t, "2026-10-01"), RunID: finished, State: doffinsync.StateDone,
		Total: int64Ptr(0), Accessible: int64Ptr(0),
	}); err != nil {
		t.Fatalf("finish day: %v", err)
	}
	if err := s.FinishRun(ctx, finished, doffinsync.RunOutcome{Reason: doffinsync.ReasonDone}); err != nil {
		t.Fatalf("finish run: %v", err)
	}
	runBefore := runState(t, pool, finished)
	dayBefore := dayState(t, pool, "2026-10-01")

	live := beginSyncRun(t, s, "2026-10-02", "2026-10-02")
	startDay(t, s, "2026-10-02", live)
	recovered, err := s.RecoverStale(ctx)
	if err != nil {
		t.Fatalf("RecoverStale: %v", err)
	}
	if recovered != 1 {
		t.Fatalf("RecoverStale marked %d runs, want 1 (the live run only)", recovered)
	}
	if got := runState(t, pool, finished); got != runBefore {
		t.Fatalf("finished run changed: %q, was %q", got, runBefore)
	}
	if got := dayState(t, pool, "2026-10-01"); got != dayBefore {
		t.Fatalf("done day changed: %q, was %q", got, dayBefore)
	}
	if got := dayState(t, pool, "2026-10-02"); got != "interrupted|process_lost" {
		t.Fatalf("live day = %q, want interrupted|process_lost", got)
	}
}

// S7 (review PG-09): a finished run is final. A second FinishRun is refused as a
// stale write, and the first outcome stands.
func TestSecondFinishRunIsRefused(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	if err := s.FinishRun(ctx, run, doffinsync.RunOutcome{
		Reason: doffinsync.ReasonDone, Counters: doffinsync.Counters{Searches: 1},
	}); err != nil {
		t.Fatalf("first FinishRun: %v", err)
	}

	err := s.FinishRun(ctx, run, doffinsync.RunOutcome{
		Reason: doffinsync.ReasonFailed, LastError: "second finish", Counters: doffinsync.Counters{Searches: 9},
	})
	if !errors.Is(err, ErrStaleWrite) {
		t.Fatalf("second FinishRun: err = %v, want ErrStaleWrite", err)
	}
	if got := runState(t, pool, run); !strings.HasPrefix(got, "done|") {
		t.Fatalf("run after the refused second finish = %q, want done", got)
	}
	if got := count(t, pool, `SELECT searches FROM sync_runs WHERE id = $1`, run); got != 1 {
		t.Fatalf("searches after the refused second finish = %d, want 1", got)
	}
}

// S6 (D-B6, migration 0003): a run's mode, window, start and id are fixed once it
// begins. Its state, end time, retry instant, last error and counters still change,
// because a run has to finish.
func TestSyncRunProvenanceIsImmutable(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-02")

	refused := []string{
		`UPDATE sync_runs SET mode = 'census' WHERE id = $1`,
		`UPDATE sync_runs SET range_from = '2026-09-30'::date WHERE id = $1`,
		`UPDATE sync_runs SET range_to = '2026-10-03'::date WHERE id = $1`,
		`UPDATE sync_runs SET started_at = started_at - interval '1 day' WHERE id = $1`,
		`UPDATE sync_runs SET id = DEFAULT WHERE id = $1`,
	}
	for _, stmt := range refused {
		_, err := pool.Exec(ctx, stmt, run)
		if pgCode(err) != sqlRestrictViolation {
			t.Errorf("%s: err = %v (SQLSTATE %q), want 23001", stmt, err, pgCode(err))
		}
	}
	if got := count(t, pool, `SELECT count(*) FROM sync_runs WHERE id = $1 AND mode = 'sync' AND range_from = '2026-10-01'::date AND range_to = '2026-10-02'::date`, run); got != 1 {
		t.Fatalf("the refused updates changed the run's provenance: %d matching row(s), want 1", got)
	}

	if _, err := pool.Exec(ctx, `
		UPDATE sync_runs
		SET state = 'partial', finished_at = now(), searches = 3, downloads = 1, stored = 1,
		    last_error = 'stopped', retry_after_until = now() + interval '1 hour'
		WHERE id = $1`, run); err != nil {
		t.Fatalf("an allowed update was refused: %v", err)
	}
}

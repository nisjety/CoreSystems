package rawstore

import (
	"testing"

	"github.com/triodelab/anbud/internal/doffinsync"
)

func int64Ptr(v int64) *int64 { return &v }

// startDay starts a day through the store and fails the test on error.
func startDay(t *testing.T, s *Store, label string, run int64) {
	t.Helper()
	if err := s.BeginDay(testCtx(t), utcDay(t, label), run); err != nil {
		t.Fatalf("BeginDay %s: %v", label, err)
	}
}

// T56: a done day must add up: held + stored + not_found equals accessible.
// The control row, which adds up, is accepted, so the refusal is about the sum.
func TestDoneRequiresAddingUp(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	startDay(t, s, "2026-10-01", run)

	bad := doffinsync.DayRecord{
		Day: utcDay(t, "2026-10-01"), RunID: run, State: doffinsync.StateDone,
		Total: int64Ptr(2), Accessible: int64Ptr(2), Stored: 1,
	}
	if err := s.FinishDay(ctx, bad); pgCode(err) != sqlCheckViolation {
		t.Fatalf("done with 1 of 2 accounted for: err = %v (SQLSTATE %q), want 23514", err, pgCode(err))
	}

	good := bad
	good.Stored = 2
	if err := s.FinishDay(ctx, good); err != nil {
		t.Fatalf("control: done with 2 of 2 accounted for was refused: %v", err)
	}
}

// T57: a done day cannot have NULL totals. NULL would pass a check that only
// compares the two counts.
func TestDoneRejectsNullCounts(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	startDay(t, s, "2026-10-01", run)

	rec := doffinsync.DayRecord{Day: utcDay(t, "2026-10-01"), RunID: run, State: doffinsync.StateDone}
	if err := s.FinishDay(ctx, rec); pgCode(err) != sqlCheckViolation {
		t.Fatalf("done with NULL totals: err = %v (SQLSTATE %q), want 23514", err, pgCode(err))
	}
}

// T58: capped means saturated, with more hits than the accessible count and no
// counted outcomes. Another code, or a total that does not exceed accessible, is refused.
func TestCappedRequiresSaturated(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	startDay(t, s, "2026-10-01", run)
	day := utcDay(t, "2026-10-01")

	wrongCode := doffinsync.DayRecord{
		Day: day, RunID: run, State: doffinsync.StateCapped, ErrorCode: doffinsync.CodeCountMismatch,
		Total: int64Ptr(5), Accessible: int64Ptr(4),
	}
	if err := s.FinishDay(ctx, wrongCode); pgCode(err) != sqlCheckViolation {
		t.Fatalf("capped with count_mismatch: err = %v (SQLSTATE %q), want 23514", err, pgCode(err))
	}

	notOver := doffinsync.DayRecord{
		Day: day, RunID: run, State: doffinsync.StateCapped, ErrorCode: doffinsync.CodeSaturated,
		Total: int64Ptr(4), Accessible: int64Ptr(4),
	}
	if err := s.FinishDay(ctx, notOver); pgCode(err) != sqlCheckViolation {
		t.Fatalf("capped with total = accessible: err = %v (SQLSTATE %q), want 23514", err, pgCode(err))
	}

	saturated := doffinsync.DayRecord{
		Day: day, RunID: run, State: doffinsync.StateCapped, ErrorCode: doffinsync.CodeSaturated,
		Total: int64Ptr(5), Accessible: int64Ptr(4),
	}
	if err := s.FinishDay(ctx, saturated); err != nil {
		t.Fatalf("control: capped with saturated was refused: %v", err)
	}
}

// T59: a running run has no finish time. Only a finished run has one.
func TestRunningRunHasNoFinish(t *testing.T) {
	_, pool := openStore(t)
	ctx := testCtx(t)

	_, err := pool.Exec(ctx, `
		INSERT INTO sync_runs (mode, range_from, range_to, state, finished_at)
		VALUES ('sync', '2026-10-01', '2026-10-01', 'running', now())`)
	if pgCode(err) != sqlCheckViolation {
		t.Fatalf("running run with finished_at: err = %v (SQLSTATE %q), want 23514", err, pgCode(err))
	}
}

// T63: each BeginDay counts an attempt and resets the day's counts. It touches
// no other day.
func TestBeginDayCountsAttempts(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-02")

	// A second day that is already done must be left alone.
	startDay(t, s, "2026-10-02", run)
	if err := s.FinishDay(ctx, doffinsync.DayRecord{
		Day: utcDay(t, "2026-10-02"), RunID: run, State: doffinsync.StateDone,
		Total: int64Ptr(0), Accessible: int64Ptr(0),
	}); err != nil {
		t.Fatalf("finish the other day: %v", err)
	}

	type dayRow struct {
		attempts              int
		state                 string
		codeNull, totalNull   bool
		held, stored, unresol int
	}
	read := func(label string) dayRow {
		var r dayRow
		err := pool.QueryRow(ctx, `
			SELECT attempts, state, error_code IS NULL, num_total IS NULL, held, stored, unresolved
			FROM sync_days WHERE source = 'doffin' AND day = $1::date`, label).
			Scan(&r.attempts, &r.state, &r.codeNull, &r.totalNull, &r.held, &r.stored, &r.unresol)
		if err != nil {
			t.Fatalf("read %s: %v", label, err)
		}
		return r
	}

	startDay(t, s, "2026-10-01", run)
	first := read("2026-10-01")
	if first.attempts != 1 {
		t.Fatalf("attempts after the first BeginDay = %d, want 1", first.attempts)
	}
	if err := s.FinishDay(ctx, doffinsync.DayRecord{
		Day: utcDay(t, "2026-10-01"), RunID: run, State: doffinsync.StatePartial, ErrorCode: doffinsync.CodeItemRejected,
		Total: int64Ptr(3), Accessible: int64Ptr(3), Held: 1, Stored: 1, Unresolved: 1,
	}); err != nil {
		t.Fatalf("finish day 1 as partial: %v", err)
	}

	startDay(t, s, "2026-10-01", run)
	again := read("2026-10-01")
	want := dayRow{attempts: 2, state: "running", codeNull: true, totalNull: true}
	if again != want {
		t.Fatalf("after the second BeginDay: %+v, want %+v", again, want)
	}
	other := read("2026-10-02")
	if other.state != "done" || other.attempts != 1 {
		t.Fatalf("the other day changed: state %s, attempts %d; want done and 1", other.state, other.attempts)
	}
}

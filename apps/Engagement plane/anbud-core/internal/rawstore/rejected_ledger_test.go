package rawstore

import (
	"testing"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// R4, R5: a done day must add up with the terminal rejections in the sum:
// held + stored + not_found + rejected equals accessible, and nothing is unresolved.
// Migration 0004 replaced the constraint, so it is the database that refuses a day
// whose sum is short or long, and that accepts one that only adds up with the new term.
func TestDoneSumIncludesRejected(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	startDay(t, s, "2026-10-01", run)
	day := utcDay(t, "2026-10-01")

	short := doffinsync.DayRecord{
		Day: day, RunID: run, State: doffinsync.StateDone,
		Total: int64Ptr(3), Accessible: int64Ptr(3), Stored: 1, Rejected: 1,
	}
	if err := s.FinishDay(ctx, short); pgCode(err) != sqlCheckViolation {
		t.Fatalf("done with 2 of 3 accounted for (1 stored, 1 rejected): err = %v (SQLSTATE %q), want 23514", err, pgCode(err))
	}

	long := doffinsync.DayRecord{
		Day: day, RunID: run, State: doffinsync.StateDone,
		Total: int64Ptr(2), Accessible: int64Ptr(2), Stored: 2, Rejected: 1,
	}
	if err := s.FinishDay(ctx, long); pgCode(err) != sqlCheckViolation {
		t.Fatalf("done with 3 of 2 accounted for (2 stored, 1 rejected): err = %v (SQLSTATE %q), want 23514", err, pgCode(err))
	}

	unresolved := doffinsync.DayRecord{
		Day: day, RunID: run, State: doffinsync.StateDone,
		Total: int64Ptr(2), Accessible: int64Ptr(2), Stored: 1, Rejected: 1, Unresolved: 1,
	}
	if err := s.FinishDay(ctx, unresolved); pgCode(err) != sqlCheckViolation {
		t.Fatalf("done with an unresolved notice: err = %v (SQLSTATE %q), want 23514", err, pgCode(err))
	}

	// The control only adds up with the new term: 1 stored of 2 is short without it.
	good := doffinsync.DayRecord{
		Day: day, RunID: run, State: doffinsync.StateDone,
		Total: int64Ptr(2), Accessible: int64Ptr(2), Stored: 1, Rejected: 1,
	}
	if err := s.FinishDay(ctx, good); err != nil {
		t.Fatalf("control: done with 1 stored + 1 rejected of 2 was refused: %v", err)
	}
	if got := count(t, pool, `SELECT rejected FROM sync_days WHERE source = 'doffin' AND day = '2026-10-01'::date`); got != 1 {
		t.Fatalf("sync_days.rejected = %d after FinishDay, want 1", got)
	}
}

// R5: a capped day counts nothing, so it must have no rejection either.
func TestCappedRequiresNoRejected(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	startDay(t, s, "2026-10-01", run)
	day := utcDay(t, "2026-10-01")

	withRejection := doffinsync.DayRecord{
		Day: day, RunID: run, State: doffinsync.StateCapped, ErrorCode: doffinsync.CodeSaturated,
		Total: int64Ptr(5), Accessible: int64Ptr(4), Rejected: 1,
	}
	if err := s.FinishDay(ctx, withRejection); pgCode(err) != sqlCheckViolation {
		t.Fatalf("capped with a rejection: err = %v (SQLSTATE %q), want 23514", err, pgCode(err))
	}

	saturated := withRejection
	saturated.Rejected = 0
	if err := s.FinishDay(ctx, saturated); err != nil {
		t.Fatalf("control: capped with no counts was refused: %v", err)
	}
}

// R5: FinishDay writes the rejected count of a day that is not done, and BeginDay
// resets it with the other counts. The column also refuses a negative count.
func TestFinishDayWritesRejectedAndBeginDayResetsIt(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	startDay(t, s, "2026-10-01", run)

	if err := s.FinishDay(ctx, doffinsync.DayRecord{
		Day: utcDay(t, "2026-10-01"), RunID: run, State: doffinsync.StatePartial, ErrorCode: doffinsync.CodeItemRejected,
		Total: int64Ptr(5), Accessible: int64Ptr(5), Held: 1, Stored: 1, Rejected: 2, Unresolved: 1,
	}); err != nil {
		t.Fatalf("finish the day as partial: %v", err)
	}
	type counts struct{ held, stored, notFound, rejected, unresolved int }
	read := func() counts {
		var c counts
		if err := pool.QueryRow(ctx, `
			SELECT held, stored, not_found, rejected, unresolved
			FROM sync_days WHERE source = 'doffin' AND day = '2026-10-01'::date`).
			Scan(&c.held, &c.stored, &c.notFound, &c.rejected, &c.unresolved); err != nil {
			t.Fatalf("read the day's counts: %v", err)
		}
		return c
	}
	if got, want := read(), (counts{held: 1, stored: 1, rejected: 2, unresolved: 1}); got != want {
		t.Fatalf("counts after FinishDay = %+v, want %+v", got, want)
	}

	startDay(t, s, "2026-10-01", run)
	if got := read(); got != (counts{}) {
		t.Fatalf("counts after the next BeginDay = %+v, want all zero: BeginDay must reset rejected", got)
	}

	_, err := pool.Exec(ctx, `UPDATE sync_days SET rejected = -1 WHERE source = 'doffin' AND day = '2026-10-01'::date`)
	if pgCode(err) != sqlCheckViolation {
		t.Fatalf("a negative rejected count: err = %v (SQLSTATE %q), want 23514", err, pgCode(err))
	}
}

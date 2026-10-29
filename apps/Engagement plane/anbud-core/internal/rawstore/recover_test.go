package rawstore

import (
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/doffinsync"
)

// T62: a run and a day left running by a killed process are marked interrupted,
// with their end set, and a second recovery finds nothing.
func TestRecoverStaleMarksRunAndDays(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	run := beginSyncRun(t, s, "2026-10-01", "2026-10-01")
	startDay(t, s, "2026-10-01", run)

	recovered, err := s.RecoverStale(ctx)
	if err != nil {
		t.Fatalf("RecoverStale: %v", err)
	}
	if recovered != 1 {
		t.Fatalf("RecoverStale marked %d runs, want 1", recovered)
	}

	var runState string
	var finished bool
	if err := pool.QueryRow(ctx, `SELECT state, finished_at IS NOT NULL FROM sync_runs WHERE id = $1`, run).
		Scan(&runState, &finished); err != nil {
		t.Fatalf("read run: %v", err)
	}
	if runState != "interrupted" || !finished {
		t.Fatalf("run state %q, finished %v; want interrupted and finished", runState, finished)
	}

	var dayState, dayCode string
	if err := pool.QueryRow(ctx, `SELECT state, error_code FROM sync_days WHERE day = '2026-10-01'::date`).
		Scan(&dayState, &dayCode); err != nil {
		t.Fatalf("read day: %v", err)
	}
	if dayState != "interrupted" || dayCode != "process_lost" {
		t.Fatalf("day state %q, code %q; want interrupted and process_lost", dayState, dayCode)
	}

	again, err := s.RecoverStale(ctx)
	if err != nil || again != 0 {
		t.Fatalf("second RecoverStale = %d, %v; want 0 and no error", again, err)
	}
}

// T68: a stored retry instant survives a restart. The latest run that stored one
// decides. The instant is returned as stored, whether or not it has passed: the
// caller compares it with its own clock.
func TestRateLimitPersistsRetryAfter(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)

	if _, ok, err := s.LastRetryAfter(ctx); err != nil || ok {
		t.Fatalf("LastRetryAfter with no runs = ok %v, err %v; want false and no error", ok, err)
	}

	finish := func(run int64, until *time.Time) {
		t.Helper()
		if err := s.FinishRun(ctx, run, doffinsync.RunOutcome{
			Reason:          doffinsync.ReasonPaused,
			LastError:       "upstream rate limit reached",
			RetryAfterUntil: until,
		}); err != nil {
			t.Fatalf("FinishRun: %v", err)
		}
	}

	future := time.Now().Add(time.Hour).UTC()
	finish(beginSyncRun(t, s, "2026-10-01", "2026-10-01"), &future)
	got, ok, err := s.LastRetryAfter(ctx)
	if err != nil || !ok {
		t.Fatalf("LastRetryAfter after a future stop = ok %v, err %v; want true", ok, err)
	}
	if !got.Equal(future.Truncate(time.Microsecond)) {
		t.Fatalf("LastRetryAfter = %v, want the stored %v", got, future)
	}

	// A later stop with a past instant replaces the stored one. It is returned as stored.
	past := time.Now().Add(-time.Hour).UTC()
	finish(beginSyncRun(t, s, "2026-10-01", "2026-10-01"), &past)
	got, ok, err = s.LastRetryAfter(ctx)
	if err != nil || !ok || !got.Equal(past.Truncate(time.Microsecond)) {
		t.Fatalf("LastRetryAfter after a past instant = %v, ok %v, err %v; want the stored past instant", got, ok, err)
	}

	// A later run without a retry instant does not hide the stored one.
	finish(beginSyncRun(t, s, "2026-10-01", "2026-10-01"), nil)
	got, ok, err = s.LastRetryAfter(ctx)
	if err != nil || !ok || !got.Equal(past.Truncate(time.Microsecond)) {
		t.Fatalf("after a run with no retry instant: %v, ok %v, err %v; want the latest stored instant", got, ok, err)
	}
}

// S4 (D-B4): the stored instant is returned as stored, with no comparison against
// the database clock. A year-old instant is still reported, so the caller can tell
// that it has passed.
func TestLastRetryAfterReturnsPastInstantAsStored(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	long := time.Date(2020, time.January, 1, 0, 0, 0, 0, time.UTC)

	if err := s.FinishRun(ctx, beginSyncRun(t, s, "2026-10-01", "2026-10-01"), doffinsync.RunOutcome{
		Reason: doffinsync.ReasonPaused, RetryAfterUntil: &long,
	}); err != nil {
		t.Fatalf("FinishRun: %v", err)
	}
	got, ok, err := s.LastRetryAfter(ctx)
	if err != nil || !ok {
		t.Fatalf("LastRetryAfter for a 2020 instant = ok %v, err %v; want true and no error", ok, err)
	}
	if !got.Equal(long) {
		t.Fatalf("LastRetryAfter = %v, want the stored %v", got, long)
	}
}

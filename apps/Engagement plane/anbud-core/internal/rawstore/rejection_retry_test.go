package rawstore

import (
	"context"
	"reflect"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

// dayCounts is one sync_days row as the ledger holds it after a run.
type dayCounts struct {
	state, code                                  string
	held, stored, notFound, rejected, unresolved int
}

func readDayCounts(t *testing.T, pool *pgxpool.Pool, label string) dayCounts {
	t.Helper()
	var c dayCounts
	if err := pool.QueryRow(testCtx(t), `
		SELECT state, COALESCE(error_code, ''), held, stored, not_found, rejected, unresolved
		FROM sync_days WHERE source = 'doffin' AND day = $1::date`, label).
		Scan(&c.state, &c.code, &c.held, &c.stored, &c.notFound, &c.rejected, &c.unresolved); err != nil {
		t.Fatalf("read day %s: %v", label, err)
	}
	return c
}

// attemptOutcomesFor lists the outcomes recorded for id, oldest first.
func attemptOutcomesFor(t *testing.T, pool *pgxpool.Pool, id string) []string {
	t.Helper()
	rows, err := pool.Query(testCtx(t), `SELECT outcome FROM download_attempts WHERE doffin_id = $1 ORDER BY id`, id)
	if err != nil {
		t.Fatalf("read attempts for %s: %v", id, err)
	}
	defer rows.Close()
	var out []string
	for rows.Next() {
		var o string
		if err := rows.Scan(&o); err != nil {
			t.Fatalf("scan attempt: %v", err)
		}
		out = append(out, o)
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("iterate attempts: %v", err)
	}
	return out
}

// R1 to R5 end to end, with the real engine, the real store and the real database,
// and one clock: the instants the engine stamps on its attempts are the instants it
// later judges them by. A notice refused for its body is held back for 24 hours,
// retried, held back again, and on its third refusal is terminal: the day is done,
// with the notice counted as rejected, and the database accepts that sum. A rerun
// makes no call, and a recheck counts the notice as rejected from its history.
func TestRejectedNoticeIsRetriedThenTerminalThroughTheRealStore(t *testing.T) {
	s, pool := openStore(t)
	const label = "2026-10-01"
	all := []string{"2026-100001", "2026-100002", "2026-100003"}
	refused := all[1]
	search := &fakeSearcher{days: map[string][]string{label: all}}

	var refusal error
	download := &fakeDownloader{respond: func(_ context.Context, id string) (doffin.Notice, error) {
		if id == refused && refusal != nil {
			return doffin.Notice{}, refusal
		}
		return noticeFor(id), nil
	}}
	runAt := func(now time.Time, recheck int) doffinsync.Summary {
		t.Helper()
		e, err := doffinsync.New(
			doffinsync.Deps{Search: search, Download: download, Pages: s, Notices: s, Runs: s, Days: s},
			doffinsync.Options{Mode: doffinsync.ModeSync, Recheck: recheck, Now: func() time.Time { return now }},
		)
		if err != nil {
			t.Fatalf("doffinsync.New: %v", err)
		}
		sum, err := e.Run(testCtx(t), window(t, label, label))
		if err != nil {
			t.Fatalf("Run at %s: %v", now.Format(time.RFC3339), err)
		}
		return sum
	}

	t0 := fixedNow

	// Run 1: the first refusal. The other two notices are stored. The day is partial.
	refusal = doffin.ErrResponseTooLarge
	if sum := runAt(t0, 0); sum.Reason != doffinsync.ReasonPartial {
		t.Fatalf("run 1 reason = %s, want partial", sum.Reason)
	}
	if want := all; !reflect.DeepEqual(download.calls, want) {
		t.Fatalf("run 1 downloaded %v, want %v", download.calls, want)
	}
	if got, want := readDayCounts(t, pool, label), (dayCounts{state: "partial", code: "item_rejected", held: 0, stored: 2, unresolved: 1}); got != want {
		t.Fatalf("day after run 1 = %+v, want %+v", got, want)
	}

	// Run 2, an hour later: the notice is cooling. No download, and no new attempt.
	callsBefore := len(download.calls)
	if sum := runAt(t0.Add(time.Hour), 0); sum.Reason != doffinsync.ReasonPartial {
		t.Fatalf("run 2 reason = %s, want partial", sum.Reason)
	}
	if len(download.calls) != callsBefore {
		t.Fatalf("run 2 downloaded %v, want no call while the notice cools", download.calls[callsBefore:])
	}
	if got := attemptOutcomesFor(t, pool, refused); !reflect.DeepEqual(got, []string{"too_large"}) {
		t.Fatalf("attempts after run 2 = %v, want only the first refusal", got)
	}

	// Run 3, 25 hours after the first refusal: retried, refused again (second refusal).
	refusal = doffin.ErrEmptyResponse
	if sum := runAt(t0.Add(25*time.Hour), 0); sum.Reason != doffinsync.ReasonPartial {
		t.Fatalf("run 3 reason = %s, want partial", sum.Reason)
	}
	if got, want := download.calls[callsBefore:], []string{refused}; !reflect.DeepEqual(got, want) {
		t.Fatalf("run 3 downloaded %v, want only %v", got, want)
	}
	if got, want := readDayCounts(t, pool, label), (dayCounts{state: "partial", code: "item_rejected", held: 2, unresolved: 1}); got != want {
		t.Fatalf("day after run 3 = %+v, want %+v", got, want)
	}

	// Run 4, exactly 24 hours after the second refusal: eligible, refused a third
	// time, so terminal in this run. The day is done and counts the notice as rejected.
	refusal = doffin.ErrResponseTooLarge
	callsBefore = len(download.calls)
	if sum := runAt(t0.Add(49*time.Hour), 0); sum.Reason != doffinsync.ReasonDone {
		t.Fatalf("run 4 reason = %s, want done", sum.Reason)
	}
	if got, want := download.calls[callsBefore:], []string{refused}; !reflect.DeepEqual(got, want) {
		t.Fatalf("run 4 downloaded %v, want only %v", got, want)
	}
	if got, want := readDayCounts(t, pool, label), (dayCounts{state: "done", held: 2, rejected: 1}); got != want {
		t.Fatalf("day after run 4 = %+v, want %+v", got, want)
	}
	if got, want := attemptOutcomesFor(t, pool, refused), []string{"too_large", "empty_body", "too_large"}; !reflect.DeepEqual(got, want) {
		t.Fatalf("attempts after run 4 = %v, want %v", got, want)
	}

	// Run 5: the day is done, so the run makes no call at all.
	callsBefore, searchesBefore := len(download.calls), search.calls
	if sum := runAt(t0.Add(50*time.Hour), 0); sum.Reason != doffinsync.ReasonDone || len(sum.Days) != 0 {
		t.Fatalf("run 5 = %s with %d day(s), want done with none", sum.Reason, len(sum.Days))
	}
	if len(download.calls) != callsBefore || search.calls != searchesBefore {
		t.Fatalf("run 5 made %d download and %d search call(s), want none",
			len(download.calls)-callsBefore, search.calls-searchesBefore)
	}

	// Run 6, with the day rechecked: the notice is terminal from its history. It is
	// counted as rejected, nothing is downloaded, and the day stays done.
	sum := runAt(t0.Add(51*time.Hour), 1)
	if sum.Reason != doffinsync.ReasonDone || len(sum.Days) != 1 || sum.Days[0].Rejected != 1 {
		t.Fatalf("run 6 = %s with days %+v, want done with one day that counts 1 rejected", sum.Reason, sum.Days)
	}
	if len(download.calls) != callsBefore {
		t.Fatalf("run 6 downloaded %v, want nothing: a terminal notice is never sent", download.calls[callsBefore:])
	}
	if got, want := readDayCounts(t, pool, label), (dayCounts{state: "done", held: 2, rejected: 1}); got != want {
		t.Fatalf("day after run 6 = %+v, want %+v", got, want)
	}
}

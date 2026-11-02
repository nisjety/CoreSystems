package rawstore

import (
	"context"
	"reflect"
	"testing"

	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

// T65: a partial run is resumed by the next run. Notices already held are not
// downloaded again, and no notice gets a second raw row.
func TestResumeSkipsDoneAndRetriesPartial(t *testing.T) {
	s, pool := openStore(t)
	ctx := testCtx(t)
	const day = "2026-10-01"
	ids := []string{"2026-100001", "2026-100002", "2026-100003"}
	search := &fakeSearcher{days: map[string][]string{day: ids}}

	// Run 1: the middle notice fails with a server error. The day is partial.
	flaky := &fakeDownloader{respond: func(_ context.Context, id string) (doffin.Notice, error) {
		if id == ids[1] {
			return doffin.Notice{}, &doffin.HTTPError{Operation: "download", Status: 500, Message: "upstream failed"}
		}
		return noticeFor(id), nil
	}}
	first, err := newSyncEngine(t, s, search, flaky).Run(ctx, window(t, day, day))
	if err != nil {
		t.Fatalf("run 1: %v", err)
	}
	if first.Reason != doffinsync.ReasonPartial {
		t.Fatalf("run 1 reason = %s, want partial", first.Reason)
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_documents`); got != 2 {
		t.Fatalf("raw rows after run 1 = %d, want 2", got)
	}

	// Run 2: the upstream recovers. Only the unresolved notice is downloaded.
	healthy := &fakeDownloader{}
	second, err := newSyncEngine(t, s, search, healthy).Run(ctx, window(t, day, day))
	if err != nil {
		t.Fatalf("run 2: %v", err)
	}
	if second.Reason != doffinsync.ReasonDone {
		t.Fatalf("run 2 reason = %s, want done", second.Reason)
	}
	if want := []string{ids[1]}; !reflect.DeepEqual(healthy.calls, want) {
		t.Fatalf("run 2 downloaded %v, want %v", healthy.calls, want)
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_documents`); got != 3 {
		t.Fatalf("raw rows after run 2 = %d, want 3", got)
	}
	if got := count(t, pool, `SELECT count(DISTINCT doffin_id) FROM raw_documents`); got != 3 {
		t.Fatalf("distinct notices = %d, want 3: a notice was stored twice", got)
	}
	if got := count(t, pool, `SELECT count(*) FROM sync_days WHERE day = '2026-10-01'::date AND state = 'done'`); got != 1 {
		t.Fatal("the day is not done after run 2")
	}
}

// T66: a run stopped mid-day keeps what it stored. The resumed run downloads only
// what is missing, which is exactly two notices here, and adds no duplicates.
func TestInterruptedRunResumesWithoutDuplicates(t *testing.T) {
	s, pool := openStore(t)
	const day = "2026-10-01"
	ids := []string{"2026-100001", "2026-100002", "2026-100003"}
	search := &fakeSearcher{days: map[string][]string{day: ids}}

	stopCtx, stop := context.WithCancel(testCtx(t))
	defer stop()
	interrupted := &fakeDownloader{respond: func(ctx context.Context, id string) (doffin.Notice, error) {
		if id == ids[1] {
			// The stop arrives while the second notice is in flight.
			stop()
			return doffin.Notice{}, context.Canceled
		}
		return noticeFor(id), nil
	}}
	first, err := newSyncEngine(t, s, search, interrupted).Run(stopCtx, window(t, day, day))
	if err != nil {
		t.Fatalf("interrupted run: %v", err)
	}
	if first.Reason != doffinsync.ReasonCancelled {
		t.Fatalf("interrupted run reason = %s, want cancelled", first.Reason)
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_documents`); got != 1 {
		t.Fatalf("raw rows after the stop = %d, want the one notice stored before it", got)
	}

	resumed := &fakeDownloader{}
	second, err := newSyncEngine(t, s, search, resumed).Run(testCtx(t), window(t, day, day))
	if err != nil {
		t.Fatalf("resumed run: %v", err)
	}
	if second.Reason != doffinsync.ReasonDone {
		t.Fatalf("resumed run reason = %s, want done", second.Reason)
	}
	if want := []string{ids[1], ids[2]}; !reflect.DeepEqual(resumed.calls, want) {
		t.Fatalf("resumed run downloaded %v, want exactly %v", resumed.calls, want)
	}
	if got := count(t, pool, `SELECT count(*) FROM raw_documents`); got != 3 {
		t.Fatalf("raw rows after resume = %d, want 3", got)
	}
	if got := count(t, pool, `SELECT count(DISTINCT doffin_id) FROM raw_documents`); got != 3 {
		t.Fatalf("distinct notices after resume = %d, want 3", got)
	}
}

// T67: a window that is already done costs nothing on rerun. No search and no
// download is made, and the run reports done with no day work.
func TestRerunOfDoneWindowMakesNoCalls(t *testing.T) {
	s, _ := openStore(t)
	ctx := testCtx(t)
	const day = "2026-10-02"
	search := &fakeSearcher{days: map[string][]string{day: {"2026-100010", "2026-100011"}}}
	download := &fakeDownloader{}
	engine := newSyncEngine(t, s, search, download)

	if sum, err := engine.Run(ctx, window(t, day, day)); err != nil || sum.Reason != doffinsync.ReasonDone {
		t.Fatalf("first run = %s, %v; want done", sum.Reason, err)
	}
	searchesBefore, downloadsBefore := search.calls, len(download.calls)

	sum, err := engine.Run(ctx, window(t, day, day))
	if err != nil {
		t.Fatalf("rerun: %v", err)
	}
	if search.calls != searchesBefore || len(download.calls) != downloadsBefore {
		t.Fatalf("rerun made %d search and %d download call(s), want none",
			search.calls-searchesBefore, len(download.calls)-downloadsBefore)
	}
	if sum.Reason != doffinsync.ReasonDone || len(sum.Days) != 0 {
		t.Fatalf("rerun reason %s with %d day(s), want done with none", sum.Reason, len(sum.Days))
	}
}

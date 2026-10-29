package main

import (
	"context"
	"errors"
	"io"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/rawstore"
)

// releaseTimeout bounds the lock release. The release runs on a context that a
// signal cannot cancel, so it needs its own bound (plan section 4, step 10).
const releaseTimeout = 10 * time.Second

// upstream is the Doffin side of a run. *doffin.Client satisfies it.
type upstream interface {
	doffinsync.Searcher
	doffinsync.Downloader
}

// runSync is one sync from the budget lock to the exit code (plan section 4,
// steps 4 to 10). The caller owns ctx and the pool. runSync releases only the
// lock it took.
func runSync(ctx context.Context, pool *pgxpool.Pool, up upstream, req request, stdout, stderr io.Writer) int {
	store, err := rawstore.NewForSource(pool, req.sourceName())
	if err != nil {
		fatalf(stderr, "the source is not one the raw store knows")
		return exitUsage
	}
	lock, err := store.TryLock(ctx)
	switch {
	case errors.Is(err, rawstore.ErrLocked):
		fatalf(stderr, "another sync holds the budget lock; nothing was called")
		return exitLocked
	case err != nil:
		fatalf(stderr, "could not take the budget lock")
		return cancelledOr(ctx, exitFailed)
	}

	// The heartbeat watches the lock for the whole run. A lost lock cancels runCtx,
	// so the run ends as cancelled instead of calling Doffin without the budget. The
	// guard refuses every Doffin call once the lock is lost, even before that cancel.
	runCtx, cancelRun := context.WithCancel(ctx)
	defer cancelRun()
	watch := watchBudget(runCtx, cancelRun, lock)
	guarded := lockGuard{up: up, lost: &watch.lost, cancel: cancelRun}

	code := syncLocked(runCtx, store, guarded, req, watch, stdout, stderr)
	lost := watch.finish()
	if err := releaseLock(ctx, lock); err != nil {
		fatalf(stderr, "could not release the budget lock cleanly; closing the session frees it")
	}
	if lost {
		fatalf(stderr, "the budget lock was lost during the run, so the run is cancelled; its session ended or released it")
		return exitCancelled
	}
	return code
}

// syncLocked runs with the budget lock held. It recovers the rows of a dead run,
// refuses to start inside a stored retry window, then runs the window. The watch
// is finished before the summary is written, so the summary and the exit agree.
func syncLocked(ctx context.Context, store *rawstore.Store, up upstream, req request, watch *budgetWatch, stdout, stderr io.Writer) int {
	rep := newReport(stdout)

	if _, err := store.RecoverStale(ctx); err != nil {
		fatalf(stderr, "could not recover rows left by an earlier run")
		return cancelledOr(ctx, exitFailed)
	}
	until, ok, err := store.LastRetryAfter(ctx)
	if err != nil {
		fatalf(stderr, "could not read the stored retry instant")
		return cancelledOr(ctx, exitFailed)
	}
	if startRefused(until, ok, time.Now()) {
		rep.emit(newSummary(0, doffinsync.ReasonPaused, doffinsync.Counters{}, &until))
		return finishReport(rep, stderr, exitPaused)
	}

	eng, err := doffinsync.New(doffinsync.Deps{
		Search:   up,
		Download: up,
		Pages:    store,
		Notices:  store,
		Runs:     store,
		Days:     store,
	}, engineOptions(req, func(d doffinsync.DayOutcome) { rep.emit(newDayLine(d)) }))
	if err != nil {
		fatalf(stderr, "sync options were refused")
		return exitUsage
	}

	sum, err := eng.Run(ctx, req.window)
	lost := watch.finish()
	reason := sum.Reason
	if err != nil {
		// The engine returns an error only when a store or the ledger failed, or the
		// context ended around one. The run did not finish cleanly, whatever its
		// last recorded reason was.
		fatalf(stderr, "sync stopped on a database or ledger error; the run row records the outcome")
		reason = doffinsync.ReasonFailed
		if ctx.Err() != nil {
			reason = doffinsync.ReasonCancelled
		}
	}
	if lost {
		reason = doffinsync.ReasonCancelled
	}
	rep.emit(newSummary(sum.RunID, reason, sum.Counters, sum.RetryAfterUntil))
	return finishReport(rep, stderr, exitCodeFor(reason))
}

// finishReport reports a broken stdout and returns the run's exit code. The run
// has already happened, so a write failure does not change the code.
func finishReport(rep *report, stderr io.Writer, code int) int {
	if rep.err != nil {
		fatalf(stderr, "could not write the report to stdout")
	}
	return code
}

// releaseLock frees the budget lock on a context that outlives a signal.
func releaseLock(parent context.Context, lock *rawstore.Lock) error {
	ctx, cancel := context.WithTimeout(context.WithoutCancel(parent), releaseTimeout)
	defer cancel()
	return lock.Release(ctx)
}

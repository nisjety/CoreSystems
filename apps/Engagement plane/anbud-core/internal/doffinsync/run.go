package doffinsync

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

const (
	defaultPageSize  = doffin.MaxHitsPerPage // the search page size of a sync (D9), the fewest searches; -page-size sets another
	maxAccessible    = 1000                  // Doffin caps numHitsAccessible here whatever the total (SOURCES.md)
	sortBy           = "PUBLICATION_DATE_ASC"
	maxSweeps        = 8 // the most sweeps of one day's pages before the day stays partial (D-B3)
	consecutiveLimit = 5 // counted upstream errors in a row that stop the run (D11)
	finishTimeout    = 10 * time.Second
)

// finishBackoff spaces the retries of the run record. The first try and one retry
// after each delay make three tries in all (D-B9).
var finishBackoff = []time.Duration{100 * time.Millisecond, 200 * time.Millisecond}

// run is the bookkeeping of one Run.
type run struct {
	e           *Engine
	ctx         context.Context
	runID       int64
	counters    Counters
	consecutive int          // counted errors since the last successful search page or download (D-B1)
	days        []DayOutcome // one entry per day written, in order
	gap         bool         // a day in the work set did not finish
}

// halt stops the run after the current day. err is set for a ledger or store
// failure, which Run returns; the other reasons are Doffin outcomes.
type halt struct {
	reason    Reason
	lastError string
	retry     *time.Time
	err       error
}

func cancelledHalt() *halt {
	return &halt{reason: ReasonCancelled, lastError: msgCancelled}
}

func keyRejectedHalt() *halt {
	return &halt{reason: ReasonKeyRejected, lastError: msgKeyRejected}
}

func ledgerFailure(op string, err error) *halt {
	return &halt{reason: ReasonFailed, lastError: msgLedger, err: fmt.Errorf("doffinsync: %s: %w", op, err)}
}

func storeFailure(op string, err error) *halt {
	return &halt{reason: ReasonFailed, lastError: msgStore, err: fmt.Errorf("doffinsync: %s: %w", op, err)}
}

// failRun turns h into a failed run that carries err. It creates h when there is none.
// When h already carries an error, both are kept, so errors.Is finds each cause.
func failRun(h *halt, err error) *halt {
	if h == nil {
		h = &halt{}
	}
	h.reason, h.lastError = ReasonFailed, msgLedger
	if h.err == nil {
		h.err = err
	} else {
		h.err = errors.Join(h.err, err)
	}
	return h
}

// runDay searches and, in sync mode, downloads one day, then writes its record.
// It returns the halt that stops the run, if any.
func (r *run) runDay(d time.Time) *halt {
	if r.ctx.Err() != nil {
		return cancelledHalt()
	}
	if err := r.e.deps.Days.BeginDay(r.ctx, d, r.runID); err != nil {
		// A write cut off by the run's own cancellation is a cancelled run, not a
		// ledger fault (E1, D-B8). Any other error stays a ledger failure.
		if r.ctx.Err() != nil {
			return cancelledHalt()
		}
		return ledgerFailure("begin day", err)
	}
	rec := DayRecord{Day: d, RunID: r.runID, State: StateRunning}
	searchesBefore := r.counters.Searches
	h := r.process(d, &rec)
	if err := r.finishDay(rec); err != nil {
		h = failRun(h, fmt.Errorf("doffinsync: finish day: %w", err))
	}
	r.recordDay(rec, r.counters.Searches-searchesBefore)
	return h
}

// process runs the day's steps and leaves rec in its final state.
func (r *run) process(d time.Time, rec *DayRecord) *halt {
	hitIDs, ended, h := r.searchDay(d, rec)
	switch {
	case h != nil || ended:
		return h
	case r.e.opts.Mode == ModeCensus:
		rec.State = StateSearched
		return nil
	}
	return r.fetchDay(d, hitIDs, rec)
}

func (r *run) finishDay(rec DayRecord) error {
	ctx, cancel := r.finishContext()
	defer cancel()
	return r.e.deps.Days.FinishDay(ctx, rec)
}

// finishContext outlives a cancelled run, so the final state of the day and of
// the run is still written.
func (r *run) finishContext() (context.Context, context.CancelFunc) {
	return context.WithTimeout(context.WithoutCancel(r.ctx), finishTimeout)
}

// recordDay reports a finished day. searches is the day's own count of search calls.
func (r *run) recordDay(rec DayRecord, searches int) {
	out := outcomeOf(rec)
	out.Searches = searches
	r.days = append(r.days, out)
	if isGap(r.e.opts.Mode, rec.State) {
		r.gap = true
	}
	if r.e.opts.Progress != nil {
		r.e.opts.Progress(out)
	}
}

// finish writes the run outcome and builds the summary. The error, if any, is
// the ledger or store failure that stopped the run, joined with a failure to
// write the outcome itself.
func (r *run) finish(h *halt) (Summary, error) {
	out := RunOutcome{Reason: ReasonDone, Counters: r.counters}
	switch {
	case h != nil:
		out.Reason, out.LastError, out.RetryAfterUntil = h.reason, h.lastError, h.retry
	case r.gap:
		out.Reason, out.LastError = ReasonPartial, msgGaps
	}
	sum := Summary{
		RunID:           r.runID,
		Reason:          out.Reason,
		Counters:        r.counters,
		Days:            r.days,
		RetryAfterUntil: out.RetryAfterUntil,
	}

	var finishErr error
	if err := r.writeOutcome(out); err != nil {
		finishErr = fmt.Errorf("doffinsync: finish run: %w", err)
	}
	var stopErr error
	if h != nil {
		stopErr = h.err
	}
	return sum, errors.Join(stopErr, finishErr)
}

// writeOutcome writes the run record. A failed write is retried after each
// delay in finishBackoff, so three tries in all, and the last error is returned
// (D-B9). The summary is still returned when every try fails.
//
// A stale write (ErrStaleWrite) after an earlier failed try means that try
// committed before it returned its error: the run is no longer running because it
// finished, so the record exists and the write is success. A stale write on the
// first try means this process never recorded the outcome, so it is an error, and
// it is not retried, because a retry of a finished run cannot succeed.
func (r *run) writeOutcome(out RunOutcome) error {
	for try := 0; ; try++ {
		err := r.writeOutcomeOnce(out)
		switch {
		case err == nil:
			return nil
		case errors.Is(err, ErrStaleWrite):
			if try == 0 {
				return err
			}
			return nil
		case try == len(finishBackoff):
			return err
		}
		time.Sleep(finishBackoff[try])
	}
}

// writeOutcomeOnce is one try at the run record, on its own finish context.
func (r *run) writeOutcomeOnce(out RunOutcome) error {
	ctx, cancel := r.finishContext()
	defer cancel()
	return r.e.deps.Runs.FinishRun(ctx, r.runID, out)
}

// isGap reports whether a finished day keeps the run from being done. A census
// has no downloads, so a searched or capped day is complete.
func isGap(mode Mode, s DayState) bool {
	if mode == ModeCensus {
		return s == StatePartial || s == StateFailed
	}
	return s != StateDone
}

func outcomeOf(rec DayRecord) DayOutcome {
	out := DayOutcome{
		Day:        rec.Day,
		State:      rec.State,
		ErrorCode:  rec.ErrorCode,
		Held:       rec.Held,
		Stored:     rec.Stored,
		NotFound:   rec.NotFound,
		Rejected:   rec.Rejected,
		Unresolved: rec.Unresolved,
	}
	if rec.Total != nil {
		out.Total = *rec.Total
	}
	if rec.Accessible != nil {
		out.Accessible = *rec.Accessible
	}
	return out
}

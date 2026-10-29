package doffinsync

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// ErrInvalidDeps and ErrInvalidOptions report a configuration the engine refuses.
var (
	ErrInvalidDeps    = errors.New("doffinsync: invalid dependencies")
	ErrInvalidOptions = errors.New("doffinsync: invalid options")
)

// Engine runs a sync or a census over a window. It makes one Doffin call at a
// time: one key is one budget.
type Engine struct {
	deps     Deps
	opts     Options
	now      func() time.Time
	pageSize int // the search page size, Options.PageSize or the default
}

// Deps are the consumer-side dependencies of an Engine. *doffin.Client satisfies
// Search and Download; the store implementations satisfy the rest.
type Deps struct {
	Search   Searcher
	Download Downloader
	Pages    PageStore
	Notices  NoticeStore
	Runs     RunLedger
	Days     DayLedger
}

// Options configure a run.
type Options struct {
	Mode         Mode
	Recheck      int              // re-search the last N days of the window even when done
	MaxDownloads int              // 0 = unlimited
	PageSize     int              // search hits per page, 1 to 100; 0 = 100
	Progress     func(DayOutcome) // optional, called after each day is recorded
	Now          func() time.Time // nil = time.Now
	// ValidID is the shape of the source's notice ids, checked before any download. nil is
	// Doffin's (doffin.IsNoticeID); the TED sync passes the TED publication number's.
	ValidID func(string) bool
}

// New rejects any nil dependency and an unknown or empty Mode, negative limits and a
// page size Doffin does not allow.
func New(d Deps, opts Options) (*Engine, error) {
	if err := d.check(); err != nil {
		return nil, err
	}
	if opts.Mode != ModeSync && opts.Mode != ModeCensus {
		return nil, fmt.Errorf("%w: mode must be sync or census", ErrInvalidOptions)
	}
	if opts.Recheck < 0 || opts.MaxDownloads < 0 {
		return nil, fmt.Errorf("%w: recheck and max downloads must not be negative", ErrInvalidOptions)
	}
	if opts.PageSize < 0 || opts.PageSize > doffin.MaxHitsPerPage {
		return nil, fmt.Errorf("%w: page size must be between 1 and %d", ErrInvalidOptions, doffin.MaxHitsPerPage)
	}
	now := opts.Now
	if now == nil {
		now = time.Now
	}
	size := opts.PageSize
	if size == 0 {
		size = defaultPageSize
	}
	return &Engine{deps: d, opts: opts, now: now, pageSize: size}, nil
}

// validID is the source's id shape, Doffin's when Options gives none.
func (e *Engine) validID(id string) bool {
	if e.opts.ValidID != nil {
		return e.opts.ValidID(id)
	}
	return doffin.IsNoticeID(id)
}

func (d Deps) check() error {
	switch {
	case d.Search == nil:
		return nilDep("Search")
	case d.Download == nil:
		return nilDep("Download")
	case d.Pages == nil:
		return nilDep("Pages")
	case d.Notices == nil:
		return nilDep("Notices")
	case d.Runs == nil:
		return nilDep("Runs")
	case d.Days == nil:
		return nilDep("Days")
	}
	return nil
}

func nilDep(name string) error {
	return fmt.Errorf("%w: %s is nil", ErrInvalidDeps, name)
}

// Run syncs or censuses the window. It returns an error only for an invalid
// window, a ledger failure or a store failure. Doffin outcomes are reported in
// Summary.Reason. Once the run has begun, the summary is returned with any error.
func (e *Engine) Run(ctx context.Context, w Window) (Summary, error) {
	if err := w.Validate(e.now()); err != nil {
		return Summary{}, err
	}
	runID, err := e.deps.Runs.BeginRun(ctx, RunStart{Mode: e.opts.Mode, From: w.From.UTC(), To: w.To.UTC()})
	if err != nil {
		return Summary{}, fmt.Errorf("doffinsync: begin run: %w", err)
	}
	r := &run{e: e, ctx: ctx, runID: runID}
	states, err := e.deps.Days.DayStates(ctx, w.From, w.To)
	if err != nil {
		// A read cut off by the run's own cancellation is a cancelled run, not a
		// ledger fault, as with the day writes (E1, D-B8).
		if ctx.Err() != nil {
			return r.finish(cancelledHalt())
		}
		return r.finish(ledgerFailure("read day states", err))
	}
	var h *halt
	for _, d := range e.workSet(w.Days(), states) {
		if h = r.runDay(d); h != nil {
			break
		}
	}
	return r.finish(h)
}

// workSet is every day that is not done, in order, plus the last Recheck days of
// the window even when done. A census skips done days entirely.
func (e *Engine) workSet(days []time.Time, states map[string]DayState) []time.Time {
	var work []time.Time
	recheckFrom := len(days) - e.opts.Recheck
	for i, d := range days {
		if states[dayKey(d)] != StateDone {
			work = append(work, d)
			continue
		}
		if e.opts.Mode == ModeSync && i >= recheckFrom {
			work = append(work, d)
		}
	}
	return work
}

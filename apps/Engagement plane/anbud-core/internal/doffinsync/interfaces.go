package doffinsync

import (
	"context"
	"errors"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// Consumer-side interfaces. Each has at most three methods. *doffin.Client
// satisfies Searcher and Downloader; internal/rawstore satisfies the others.

// Searcher runs one search page.
type Searcher interface {
	Search(ctx context.Context, p doffin.SearchParams) (doffin.SearchResponse, error)
}

// Downloader fetches one notice.
type Downloader interface {
	Download(ctx context.Context, doffinID string) (doffin.Notice, error)
}

// PageStore keeps search response bodies.
type PageStore interface {
	PutSearchPage(ctx context.Context, runID int64, p SearchPage) error
}

// NoticeStore keeps notice bodies and download outcomes.
type NoticeStore interface {
	// Resolve reports what the store knows about each id, as facts. It applies no
	// retry policy: the engine decides, with its own clock, whether a rejected id
	// is cooling, eligible again or terminal. An id with nothing to report is absent.
	//
	// Precedence: a held id (a raw row exists) beats everything. Otherwise the most
	// recent definitive attempt decides between not found and rejected. The
	// definitive outcomes are not_found, too_large and empty_body. The transient
	// outcomes (http_error, transport_error, rate_limited, unauthorized, invalid_id)
	// are ignored. For a rejected id, Rejections is the number of rejected attempts
	// it has ever had and LastRejection is the attempted_at of the latest one; the
	// host clock wrote that instant, and the engine compares it with its own clock.
	Resolve(ctx context.Context, ids []string) (map[string]Resolution, error)
	// StoreNotice writes the raw row and its stored or duplicate attempt in one transaction.
	StoreNotice(ctx context.Context, runID int64, n doffin.Notice) (inserted bool, err error)
	RecordAttempt(ctx context.Context, runID int64, a Attempt) error
}

// ErrStaleWrite is what a ledger wraps when a write changed no row because its run
// is no longer running. A FinishRun that returns it after an earlier try failed
// means that earlier try committed, so the run record exists (D-B9).
var ErrStaleWrite = errors.New("doffinsync: write refused, its run is no longer running")

// RunLedger records runs.
type RunLedger interface {
	BeginRun(ctx context.Context, r RunStart) (runID int64, err error)
	FinishRun(ctx context.Context, runID int64, out RunOutcome) error
}

// DayLedger records per-day state.
type DayLedger interface {
	// DayStates is keyed "2006-01-02" for every day in [from, to] that has a row.
	DayStates(ctx context.Context, from, to time.Time) (map[string]DayState, error)
	BeginDay(ctx context.Context, day time.Time, runID int64) error
	FinishDay(ctx context.Context, rec DayRecord) error
}

var _ Searcher = (*doffin.Client)(nil)
var _ Downloader = (*doffin.Client)(nil)

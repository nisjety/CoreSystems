// Package rawstore is the Postgres side of the Doffin sync. It implements the
// consumer-side store interfaces of doffinsync with pgx, over the migration 0002
// tables. The evidence tables are append-only: this package only inserts into
// them (TestRawstoreNeverMutatesEvidence checks that).
package rawstore

import (
	"errors"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/noticeid"
)

var (
	_ doffinsync.PageStore   = (*Store)(nil)
	_ doffinsync.NoticeStore = (*Store)(nil)
	_ doffinsync.RunLedger   = (*Store)(nil)
	_ doffinsync.DayLedger   = (*Store)(nil)
)

// Refusals. Each is returned before any SQL runs, or wrapped around a database error.
var (
	// ErrInvalidNotice refuses a notice whose id shape, body or hash the store will not keep.
	ErrInvalidNotice = errors.New("rawstore: invalid notice")
	// ErrInvalidAttempt refuses an attempt row the store will not write.
	ErrInvalidAttempt = errors.New("rawstore: invalid attempt")
	// ErrInvalidPage refuses a search page the store will not keep.
	ErrInvalidPage = errors.New("rawstore: invalid search page")
	// ErrInvalidRecord refuses a run or day record with a value the ledger does not allow.
	ErrInvalidRecord = errors.New("rawstore: invalid ledger record")
)

// ErrStaleWrite refuses a write whose run is no longer running: recovery
// interrupted it, or it has already finished. Such a write is never kept, so a
// process that outlived its run cannot change the run or day ledger, or add a
// search page, a notice body or a download attempt, after recovery has run. A ledger
// write that changed no row returns it. An evidence write that migration 0005's
// run guard refused (SQLSTATE 55000) returns an error that wraps it and keeps the
// database error, which names the table and the run id and carries no row data.
//
// It is the same value as doffinsync.ErrStaleWrite, so the engine can recognise a
// refused FinishRun with errors.Is (D-B9).
var ErrStaleWrite = doffinsync.ErrStaleWrite

// Store writes the raw corpus and the sync ledger of one source, and reads the corpus of every
// source. It holds no state beyond the pool and its source.
//
// What a sync writes (notices, attempts, search pages, the day ledger) and what it resolves are the
// store's own source. What the readers return (RecentNotices, Notice, HasNotice, the catalogue) is
// every source, each notice under its public id (internal/noticeid), so a reader never has to know
// where a notice came from.
type Store struct {
	pool   *pgxpool.Pool
	source string
}

// ErrUnknownSource refuses a source the raw store has no rows for (migration 0008).
var ErrUnknownSource = errors.New("rawstore: unknown source")

// New returns the Doffin store over pool. It does not check the pool.
func New(pool *pgxpool.Pool) *Store {
	return &Store{pool: pool, source: noticeid.SourceDoffin}
}

// NewForSource returns the store of source over pool: noticeid.SourceDoffin, SourceTED, SourceBOAMP, SourceFTS or SourceBZP.
func NewForSource(pool *pgxpool.Pool, source string) (*Store, error) {
	switch source {
	case noticeid.SourceDoffin, noticeid.SourceTED, noticeid.SourceBOAMP, noticeid.SourceFTS, noticeid.SourceBZP:
	default:
		return nil, ErrUnknownSource
	}
	return &Store{pool: pool, source: source}, nil
}

// Source is the source the store writes.
func (s *Store) Source() string { return s.source }

const (
	// maxContentType is the content_type bound of migration 0002.
	maxContentType = 255
	// maxAttemptMessage is the download_attempts.message bound.
	maxAttemptMessage = 256
	// maxLastError is the sync_runs.last_error bound.
	maxLastError = 200
	// maxQueryKey is the raw_search_pages.query_key bound.
	maxQueryKey = 300
	// dayLayout is the label of a sync day, the form DayStates keys by.
	dayLayout = "2006-01-02"
)

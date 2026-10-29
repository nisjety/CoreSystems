package rawstore

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/anbud/internal/noticeid"
)

// MaxRecent bounds how many rows RecentNotices reads in one call.
const MaxRecent = 1000

var (
	// ErrInvalidQuery refuses a read the store will not run: a limit outside 1 to MaxRecent, or an
	// id that does not have the shape of a notice id. It is returned before any SQL runs.
	ErrInvalidQuery = errors.New("rawstore: invalid read")
	// ErrNotFound means no body of the notice is held and live: it was never stored, or every body
	// of it has been erased.
	ErrNotFound = errors.New("rawstore: notice not found")
)

// Document is a held notice body with the evidence about it: which notice, the hash of the
// bytes, and when they were fetched.
type Document struct {
	ID        string // the public id (internal/noticeid): a Doffin id, or TED- and a TED publication number
	Source    string
	Body      []byte
	SHA256    string
	FetchedAt time.Time // UTC
}

// The readers read raw_documents_live, never raw_documents: a body that has been erased is not a
// body any more, and a reader that honours the redaction list is how it stays erased
// (docs/PERSONAL-DATA.md, section 6). Rows are ordered by id, the primary key, which grows with
// insertion: the newest stored body is the one with the highest id.
const selectRecentNotices = `
SELECT source, doffin_id, body, body_sha256, fetched_at
  FROM raw_documents_live
 ORDER BY id DESC
 LIMIT $1`

const selectAllNotices = `
SELECT source, doffin_id, body, body_sha256, fetched_at
  FROM raw_documents_live
 ORDER BY id DESC`

const selectNotice = `
SELECT source, doffin_id, body, body_sha256, fetched_at
  FROM raw_documents_live
 WHERE source = $1 AND doffin_id = $2
 ORDER BY id DESC
 LIMIT 1`

const selectHasNotice = `
SELECT EXISTS (SELECT 1 FROM raw_documents_live WHERE source = $1 AND doffin_id = $2)`

// HasNotice reports whether a live body of the notice is held, without reading it. A body that has
// been erased does not count.
func (s *Store) HasNotice(ctx context.Context, id string) (bool, error) {
	source, sourceID, ok := noticeid.Parse(id)
	if !ok {
		return false, fmt.Errorf("%w: the notice id has an invalid shape", ErrInvalidQuery)
	}
	var held bool
	if err := s.pool.QueryRow(ctx, selectHasNotice, source, sourceID).Scan(&held); err != nil {
		return false, fmt.Errorf("rawstore: look for notice: %w", err)
	}
	return held, nil
}

// RecentNotices calls fn for the newest limit stored bodies, newest first, one body per notice:
// when a notice has changed, its latest live body is the one given. The bodies are streamed, so a
// caller that does not keep them holds one at a time. An error from fn stops the read and is
// returned as it is. The limit is a number of stored rows, not of notices: a notice with several
// bodies in the window counts once for each.
func (s *Store) RecentNotices(ctx context.Context, limit int, fn func(Document) error) error {
	if limit < 1 || limit > MaxRecent {
		return fmt.Errorf("%w: the limit must be between 1 and %d", ErrInvalidQuery, MaxRecent)
	}
	rows, err := s.pool.Query(ctx, selectRecentNotices, limit)
	if err != nil {
		return fmt.Errorf("rawstore: read recent notices: %w", err)
	}
	return s.eachNotice(rows, limit, fn)
}

// AllNotices calls fn for every stored body, newest first, one body per notice, with no bound on the
// count. It streams: a body is handed on as it is read, so the store is never held in memory. A profile
// suggestion reads the whole store, since the wins an organisation has are not all in the newest rows.
func (s *Store) AllNotices(ctx context.Context, fn func(Document) error) error {
	rows, err := s.pool.Query(ctx, selectAllNotices)
	if err != nil {
		return fmt.Errorf("rawstore: read all notices: %w", err)
	}
	return s.eachNotice(rows, 0, fn)
}

// eachNotice hands each document of rows to fn, skipping a second body of a notice already seen.
func (s *Store) eachNotice(rows pgx.Rows, size int, fn func(Document) error) error {
	defer rows.Close()

	seen := make(map[string]bool, size)
	for rows.Next() {
		d, err := scanDocument(rows)
		if err != nil {
			return err
		}
		if seen[d.ID] {
			continue
		}
		seen[d.ID] = true
		if err := fn(d); err != nil {
			return err
		}
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("rawstore: read notices: %w", err)
	}
	return nil
}

// Notice returns the latest live body of one notice, or ErrNotFound.
func (s *Store) Notice(ctx context.Context, id string) (Document, error) {
	source, sourceID, ok := noticeid.Parse(id)
	if !ok {
		return Document{}, fmt.Errorf("%w: the notice id has an invalid shape", ErrInvalidQuery)
	}
	rows, err := s.pool.Query(ctx, selectNotice, source, sourceID)
	if err != nil {
		return Document{}, fmt.Errorf("rawstore: read notice: %w", err)
	}
	defer rows.Close()
	if !rows.Next() {
		if err := rows.Err(); err != nil {
			return Document{}, fmt.Errorf("rawstore: read notice: %w", err)
		}
		return Document{}, ErrNotFound
	}
	return scanDocument(rows)
}

func scanDocument(rows pgx.Rows) (Document, error) {
	var d Document
	if err := rows.Scan(&d.Source, &d.ID, &d.Body, &d.SHA256, &d.FetchedAt); err != nil {
		return Document{}, fmt.Errorf("rawstore: scan notice: %w", err)
	}
	d.ID = noticeid.Public(d.Source, d.ID)
	d.FetchedAt = d.FetchedAt.UTC()
	return d, nil
}

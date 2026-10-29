package rawstore

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/noticeid"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

const insertRawDocument = `
INSERT INTO raw_documents (source, doffin_id, content_type, body, body_sha256, fetched_at, run_id)
VALUES ($1, $2, $3, $4, $5, $6, $7)
ON CONFLICT (source, doffin_id, body_sha256) DO NOTHING
RETURNING id`

const selectRawDocument = `
SELECT id FROM raw_documents
WHERE source = $1 AND doffin_id = $2 AND body_sha256 = $3`

const insertAttempt = `
INSERT INTO download_attempts (run_id, source, doffin_id, attempted_at, outcome, http_status, message, raw_document_id)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`

// attemptOutcomes are the download_attempts outcomes RecordAttempt may write.
// stored and duplicate are absent: StoreNotice writes those with their raw row.
var attemptOutcomes = map[string]bool{
	doffinsync.OutcomeNotFound:       true,
	doffinsync.OutcomeInvalidID:      true,
	doffinsync.OutcomeHTTPError:      true,
	doffinsync.OutcomeEmptyBody:      true,
	doffinsync.OutcomeTooLarge:       true,
	doffinsync.OutcomeTransportError: true,
	doffinsync.OutcomeRateLimited:    true,
	doffinsync.OutcomeUnauthorized:   true,
}

// StoreNotice writes a notice body and its stored or duplicate attempt in one
// transaction. Either both rows commit or neither does. The bool reports whether
// the body was new. A run that is no longer running cannot take evidence: the
// database refuses the row, and the error wraps ErrStaleWrite.
func (s *Store) StoreNotice(ctx context.Context, runID int64, n doffin.Notice) (bool, error) {
	if err := validateNotice(s.source, n); err != nil {
		return false, err
	}
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return false, fmt.Errorf("rawstore: begin notice transaction: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	inserted, id, err := insertNotice(ctx, tx, s.source, runID, n)
	if err != nil {
		return false, err
	}
	outcome := doffinsync.OutcomeDuplicate
	if inserted {
		outcome = doffinsync.OutcomeStored
	}
	if _, err := tx.Exec(ctx, insertAttempt,
		runID, s.source, n.ID, n.FetchedAt.UTC(), outcome, nil, nil, id); err != nil {
		return false, fmt.Errorf("rawstore: record %s attempt: %w", outcome, staleOr(err))
	}
	if err := tx.Commit(ctx); err != nil {
		return false, fmt.Errorf("rawstore: commit notice: %w", err)
	}
	return inserted, nil
}

// insertNotice writes the body, or finds the row that already holds the same
// bytes. ON CONFLICT DO NOTHING returns no row on a duplicate, so the second
// query fetches the existing id.
func insertNotice(ctx context.Context, tx pgx.Tx, source string, runID int64, n doffin.Notice) (bool, int64, error) {
	var id int64
	err := tx.QueryRow(ctx, insertRawDocument,
		source, n.ID, cleanText(n.ContentType, maxContentType),
		n.Body, n.SHA256, n.FetchedAt.UTC(), runID).Scan(&id)
	if err == nil {
		return true, id, nil
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return false, 0, fmt.Errorf("rawstore: insert notice body: %w", staleOr(err))
	}
	if err := tx.QueryRow(ctx, selectRawDocument, source, n.ID, n.SHA256).Scan(&id); err != nil {
		return false, 0, fmt.Errorf("rawstore: find held notice body: %w", err)
	}
	return false, id, nil
}

// RecordAttempt writes a download outcome other than stored or duplicate. It
// holds no body, so it has no raw_document_id. An HTTP status outside 100 to 599
// is stored as NULL and does not refuse the attempt. A run that is no longer
// running cannot take evidence: the database refuses the row, and the error wraps
// ErrStaleWrite.
func (s *Store) RecordAttempt(ctx context.Context, runID int64, a doffinsync.Attempt) error {
	if err := validateAttempt(s.source, a); err != nil {
		return err
	}
	var message any // NULL when there is no message
	if m := cleanText(a.Message, maxAttemptMessage); m != "" {
		message = m
	}
	if _, err := s.pool.Exec(ctx, insertAttempt,
		runID, s.source, a.DoffinID, a.At.UTC(), a.Outcome, httpStatusValue(a.HTTPStatus), message, nil); err != nil {
		return fmt.Errorf("rawstore: record %s attempt: %w", a.Outcome, staleOr(err))
	}
	return nil
}

// httpStatusValue is the http_status stored for an attempt: the status when it is
// a valid HTTP status, NULL otherwise. Zero, which means no status, is NULL too.
func httpStatusValue(status int) any {
	if status < 100 || status > 599 {
		return nil
	}
	return status
}

// validateNotice applies the rules migration 0002 checks, before any SQL runs.
// It recomputes the hash from the bytes: a caller's digest is never trusted, and
// an uppercase digest is refused, as the database refuses it. The error never
// echoes the id, because an id from upstream is data, not a value to log.
func validateNotice(source string, n doffin.Notice) error {
	if !noticeid.ValidFor(source, n.ID) {
		return fmt.Errorf("%w: notice id has an invalid shape", ErrInvalidNotice)
	}
	if len(n.Body) == 0 {
		return fmt.Errorf("%w: body is empty", ErrInvalidNotice)
	}
	sum := sha256.Sum256(n.Body)
	if n.SHA256 != hex.EncodeToString(sum[:]) {
		return fmt.Errorf("%w: sha256 is not the lower-case hex digest of the body", ErrInvalidNotice)
	}
	if n.FetchedAt.IsZero() {
		return fmt.Errorf("%w: fetch time is missing", ErrInvalidNotice)
	}
	return nil
}

// validateAttempt refuses an attempt the download_attempts checks would refuse,
// and the two outcomes that only StoreNotice may write. The HTTP status is not
// checked here: an out-of-range status is stored as NULL instead.
func validateAttempt(source string, a doffinsync.Attempt) error {
	switch {
	case a.Outcome == doffinsync.OutcomeStored || a.Outcome == doffinsync.OutcomeDuplicate:
		return fmt.Errorf("%w: stored and duplicate are written by StoreNotice", ErrInvalidAttempt)
	case !attemptOutcomes[a.Outcome]:
		return fmt.Errorf("%w: outcome is not one that RecordAttempt writes", ErrInvalidAttempt)
	case !noticeid.ValidFor(source, a.DoffinID):
		return fmt.Errorf("%w: notice id has an invalid shape", ErrInvalidAttempt)
	case a.At.IsZero():
		return fmt.Errorf("%w: attempt time is missing", ErrInvalidAttempt)
	}
	return nil
}

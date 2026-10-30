package rawstore

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"unicode/utf8"

	"github.com/triodelab/anbud/internal/doffinsync"
)

const insertSearchPage = `
INSERT INTO raw_search_pages (source, query_key, page, content_type, body, body_sha256, fetched_at, run_id)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
ON CONFLICT (source, query_key, page, body_sha256) DO NOTHING`

// PutSearchPage keeps one search response body, with its content type and fetch
// time. The bytes are stored exactly, even when they do not decode. An identical
// page already held is left as it is. A run that is no longer running cannot take
// evidence: the database refuses the row, even a duplicate one, and the error wraps
// ErrStaleWrite.
func (s *Store) PutSearchPage(ctx context.Context, runID int64, p doffinsync.SearchPage) error {
	if err := validatePage(p); err != nil {
		return err
	}
	sum := sha256.Sum256(p.Body)
	if _, err := s.pool.Exec(ctx, insertSearchPage,
		s.source, p.QueryKey, p.Page, cleanText(p.ContentType, maxContentType),
		p.Body, hex.EncodeToString(sum[:]), p.FetchedAt.UTC(), runID); err != nil {
		return fmt.Errorf("rawstore: store search page %d: %w", p.Page, staleOr(err))
	}
	return nil
}

// validatePage applies the raw_search_pages checks before any SQL runs.
func validatePage(p doffinsync.SearchPage) error {
	keyLen := utf8.RuneCountInString(p.QueryKey)
	switch {
	case len(p.Body) == 0:
		return fmt.Errorf("%w: body is empty", ErrInvalidPage)
	case p.Page < 1:
		return fmt.Errorf("%w: page must be at least 1", ErrInvalidPage)
	case keyLen < 1 || keyLen > maxQueryKey:
		return fmt.Errorf("%w: query key must be 1 to %d characters", ErrInvalidPage, maxQueryKey)
	case p.FetchedAt.IsZero():
		return fmt.Errorf("%w: fetch time is missing", ErrInvalidPage)
	}
	return nil
}

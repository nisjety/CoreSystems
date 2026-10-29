package rawstore

import (
	"context"
	"fmt"

	"github.com/triodelab/anbud/internal/noticeid"
)

// CatalogueDocument carries the insertion cursor of the latest live notice body.
type CatalogueDocument struct {
	Sequence int64
	Document Document
}

// CataloguePage traverses the entire live corpus, every source, without returning superseded bodies:
// a newer body of the same notice of the same source supersedes. Redacted bodies remain excluded by
// raw_documents_live, including in the newer-body check. Each document carries its public id.
func (s *Store) CataloguePage(ctx context.Context, before int64, limit int) ([]CatalogueDocument, error) {
	if before < 0 || limit < 1 || limit > 100 {
		return nil, fmt.Errorf("%w: invalid catalogue page", ErrInvalidQuery)
	}
	rows, err := s.pool.Query(ctx, `SELECT d.id, d.source, d.doffin_id, d.body, d.body_sha256, d.fetched_at
 FROM raw_documents_live d WHERE ($1::bigint=0 OR d.id<$1)
 AND NOT EXISTS (SELECT 1 FROM raw_documents_live newer WHERE newer.source=d.source AND newer.doffin_id=d.doffin_id AND newer.id>d.id)
 ORDER BY d.id DESC LIMIT $2`, before, limit)
	if err != nil {
		return nil, fmt.Errorf("rawstore: catalogue: %w", err)
	}
	defer rows.Close()
	out := make([]CatalogueDocument, 0, limit)
	for rows.Next() {
		var d CatalogueDocument
		if err := rows.Scan(&d.Sequence, &d.Document.Source, &d.Document.ID, &d.Document.Body, &d.Document.SHA256, &d.Document.FetchedAt); err != nil {
			return nil, err
		}
		d.Document.ID = noticeid.Public(d.Document.Source, d.Document.ID)
		d.Document.FetchedAt = d.Document.FetchedAt.UTC()
		out = append(out, d)
	}
	return out, rows.Err()
}

// CatalogueTotal counts distinct held notices of every source; body revisions and redactions do not
// inflate it. A notice published both on Doffin and on TED counts once per source.
func (s *Store) CatalogueTotal(ctx context.Context) (int, error) {
	var total int
	err := s.pool.QueryRow(ctx, `SELECT count(*) FROM (SELECT DISTINCT source, doffin_id FROM raw_documents_live) held`).Scan(&total)
	return total, err
}

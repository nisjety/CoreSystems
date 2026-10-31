package rawstore

import (
	"context"
	"encoding/json"
	"github.com/jackc/pgx/v5"
	"github.com/triodelab/anbud/internal/noticeid"
	"time"
)

type CatalogueProjection struct {
	Sequence        int64
	Payload         json.RawMessage
	MatchingPayload json.RawMessage
	ActiveUntil     *time.Time
}

func (s *Store) CatalogueGeneration(ctx context.Context) (int64, error) {
	var generation int64
	err := s.pool.QueryRow(ctx, `SELECT generation FROM catalogue_generation WHERE singleton`).Scan(&generation)
	return generation, err
}

// PendingCatalogue reads only bodies lacking the current projection. Superseded and
// erased bodies are never parsed by the preparer.
func (s *Store) PendingCatalogue(ctx context.Context, version int) ([]CatalogueDocument, error) {
	rows, err := s.pool.Query(ctx, `SELECT d.id,d.source,d.doffin_id,d.body,d.body_sha256,d.fetched_at
 FROM raw_documents_live d LEFT JOIN catalogue_projection p ON p.raw_document_id=d.id AND p.parser_version=$1
 WHERE p.raw_document_id IS NULL AND NOT EXISTS
 (SELECT 1 FROM raw_documents_live n WHERE n.source=d.source AND n.doffin_id=d.doffin_id AND n.id>d.id)
 ORDER BY d.id LIMIT 100`, version)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []CatalogueDocument{}
	for rows.Next() {
		var d CatalogueDocument
		if err := rows.Scan(&d.Sequence, &d.Document.Source, &d.Document.ID, &d.Document.Body, &d.Document.SHA256, &d.Document.FetchedAt); err != nil {
			return nil, err
		}
		d.Document.ID = noticeid.Public(d.Document.Source, d.Document.ID)
		out = append(out, d)
	}
	return out, rows.Err()
}

// A source row lock serialises projection insertion with erasure. A concurrent erasure
// either wins first (no insert), or deletes the inserted projection in its transaction.
func (s *Store) SaveCatalogueProjection(ctx context.Context, version int, entries []CatalogueProjection) error {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer tx.Rollback(ctx)
	batch := &pgx.Batch{}
	for _, entry := range entries {
		batch.Queue(`WITH held AS (SELECT id FROM raw_documents WHERE id=$1 AND redacted_at IS NULL)
 INSERT INTO catalogue_projection(raw_document_id,parser_version,payload,matching_payload,active_until)
 SELECT id,$2,$3::jsonb,$4::jsonb,$5 FROM held ON CONFLICT(raw_document_id) DO UPDATE SET parser_version=excluded.parser_version,payload=excluded.payload,matching_payload=excluded.matching_payload,active_until=excluded.active_until`, entry.Sequence, version, entry.Payload, entry.MatchingPayload, entry.ActiveUntil)
	}
	result := tx.SendBatch(ctx, batch)
	if err := result.Close(); err != nil {
		return err
	}
	return tx.Commit(ctx)
}

func (s *Store) PreparedCatalogue(ctx context.Context, version int, revision int64) ([]CatalogueProjection, error) {
	rows, err := s.pool.Query(ctx, `SELECT d.id,p.payload FROM raw_documents_live d
 JOIN catalogue_projection p ON p.raw_document_id=d.id AND p.parser_version=$1
 WHERE ($2::bigint=0 OR d.id<=$2) AND NOT EXISTS
 (SELECT 1 FROM raw_documents_live n WHERE n.source=d.source AND n.doffin_id=d.doffin_id AND n.id>d.id)
 ORDER BY d.id DESC`, version, revision)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []CatalogueProjection{}
	for rows.Next() {
		var p CatalogueProjection
		if err := rows.Scan(&p.Sequence, &p.Payload); err != nil {
			return nil, err
		}
		out = append(out, p)
	}
	return out, rows.Err()
}

func (s *Store) SetCataloguePublicationDates(ctx context.Context, version int, dates map[string]string) error {
	body, err := json.Marshal(dates)
	if err != nil {
		return err
	}
	_, err = s.pool.Exec(ctx, `UPDATE catalogue_projection p SET payload=jsonb_set(p.payload,'{notice,published_date}',to_jsonb(coalesce(dates.value,'')))
 FROM raw_documents_live d LEFT JOIN jsonb_each_text($2::jsonb) dates
 ON dates.key=CASE WHEN d.source='doffin' THEN d.doffin_id ELSE upper(d.source)||'-'||d.doffin_id END
 WHERE d.id=p.raw_document_id AND p.parser_version=$1
 AND coalesce(p.payload#>>'{notice,published_date}','')<>coalesce(dates.value,'')`, version, body)
	return err
}

func (s *Store) CatalogueSourceGeneration(ctx context.Context) (int64, error) {
	var generation int64
	err := s.pool.QueryRow(ctx, `SELECT source_generation FROM catalogue_generation WHERE singleton`).Scan(&generation)
	return generation, err
}

// Inactive notices contribute only lifecycle identities. This prevents an old,
// open version resurfacing after a newer amendment closes a procedure.
func (s *Store) MatchingCatalogue(ctx context.Context, version int, now time.Time) ([]CatalogueProjection, error) {
	rows, err := s.pool.Query(ctx, `SELECT d.id,p.payload->'matching_identity',CASE WHEN $2::timestamptz='0001-01-01T00:00:00Z'::timestamptz OR p.active_until >= $2 THEN p.matching_payload END
 FROM catalogue_projection p JOIN raw_documents_live d ON d.id=p.raw_document_id
 WHERE p.parser_version=$1 AND p.matching_payload IS NOT NULL AND NOT EXISTS
 (SELECT 1 FROM raw_documents_live newer WHERE newer.source=d.source AND newer.doffin_id=d.doffin_id AND newer.id>d.id)
 ORDER BY d.id DESC`, version, now)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []CatalogueProjection{}
	for rows.Next() {
		var p CatalogueProjection
		if err := rows.Scan(&p.Sequence, &p.Payload, &p.MatchingPayload); err != nil {
			return nil, err
		}
		out = append(out, p)
	}
	return out, rows.Err()
}

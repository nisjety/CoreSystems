package rawstore

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/noticeid"
)

const selectHeldIDs = `
SELECT DISTINCT doffin_id FROM raw_documents
WHERE source = $1 AND doffin_id = ANY($2::text[])`

// selectDefinitive reads what the definitive attempts of each id say. The definitive
// outcomes are not_found, too_large and empty_body; every other outcome (http_error,
// transport_error, rate_limited, unauthorized, invalid_id, and the stored and
// duplicate outcomes that come with a raw row) is not read at all, so it cannot
// decide anything. Per id it returns the outcome and the attempted_at of the most
// recent definitive attempt, which is the highest id because attempts are only
// appended, and the number of rejected attempts (too_large or empty_body) the id
// has ever had.
const selectDefinitive = `
SELECT doffin_id,
       (array_agg(outcome ORDER BY id DESC))[1],
       (array_agg(attempted_at ORDER BY id DESC))[1],
       count(*) FILTER (WHERE outcome IN ('too_large', 'empty_body'))
FROM download_attempts
WHERE source = $1 AND doffin_id = ANY($2::text[])
  AND outcome IN ('not_found', 'too_large', 'empty_body')
GROUP BY doffin_id`

// Resolve reports what the store knows about each id, as facts. It applies no retry
// policy and compares no time: the engine decides, with its own clock, whether a
// rejected id is cooling, eligible again or terminal.
//
// A held id (a raw row exists) beats everything. Otherwise the most recent
// definitive attempt decides between not found and rejected, so a notice that
// returned 404 earlier but was refused for its body later is rejected, not not
// found. A transient outcome is ignored. For a rejected id, Rejections is the number
// of rejected attempts it has ever had and LastRejection is the attempted_at of the
// latest one. Neither is set for a held or a not found id. An id with nothing to
// report is absent.
//
// Only ids of notice shape are looked up: the tables hold no other ids, so any
// other id is unresolved by definition, and a NUL byte in one must not fail the
// lookup.
func (s *Store) Resolve(ctx context.Context, ids []string) (map[string]doffinsync.Resolution, error) {
	out := make(map[string]doffinsync.Resolution, len(ids))
	candidates := make([]string, 0, len(ids))
	for _, id := range ids {
		if noticeid.ValidFor(s.source, id) {
			candidates = append(candidates, id)
		}
	}
	if len(candidates) == 0 {
		return out, nil
	}
	held, err := s.matchingIDs(ctx, selectHeldIDs, candidates)
	if err != nil {
		return nil, fmt.Errorf("rawstore: resolve held notices: %w", err)
	}
	for _, id := range held {
		out[id] = doffinsync.Resolution{Kind: doffinsync.ResolutionHeld}
	}
	if err := s.resolveDefinitive(ctx, candidates, out); err != nil {
		return nil, err
	}
	return out, nil
}

// resolveDefinitive adds the not found and rejected verdicts to out, for the ids
// that are not held.
func (s *Store) resolveDefinitive(ctx context.Context, ids []string, out map[string]doffinsync.Resolution) error {
	rows, err := s.pool.Query(ctx, selectDefinitive, s.source, ids)
	if err != nil {
		return fmt.Errorf("rawstore: resolve definitive attempts: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var (
			id, latest string
			latestAt   time.Time
			rejections int64
		)
		if err := rows.Scan(&id, &latest, &latestAt, &rejections); err != nil {
			return fmt.Errorf("rawstore: scan definitive attempts: %w", err)
		}
		if _, held := out[id]; held {
			continue
		}
		switch latest {
		case doffinsync.OutcomeNotFound:
			out[id] = doffinsync.Resolution{Kind: doffinsync.ResolutionNotFound}
		case doffinsync.OutcomeTooLarge, doffinsync.OutcomeEmptyBody:
			out[id] = doffinsync.Resolution{
				Kind:          doffinsync.ResolutionRejected,
				Rejections:    int(rejections),
				LastRejection: latestAt.UTC(),
			}
		}
	}
	if err := rows.Err(); err != nil {
		return fmt.Errorf("rawstore: iterate definitive attempts: %w", err)
	}
	return nil
}

func (s *Store) matchingIDs(ctx context.Context, query string, ids []string) ([]string, error) {
	rows, err := s.pool.Query(ctx, query, s.source, ids)
	if err != nil {
		return nil, err
	}
	return pgx.CollectRows(rows, pgx.RowTo[string])
}

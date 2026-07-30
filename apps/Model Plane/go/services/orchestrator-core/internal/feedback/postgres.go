package feedback

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// PostgresStore is the durable rating sink. One row per
// (org_id, run_id, user_id, skill_id); a re-rating UPSERTs, so a user who clicks
// thumbs-up then thumbs-down leaves one row with the latest value rather than
// two contradictory samples.
//
// Aggregation is a SQL GROUP BY at read time so detail is never lost and the
// promotion bar can be re-derived with different parameters.
type PostgresStore struct {
	pool *pgxpool.Pool
}

// NewPostgresStore constructs a Store from a connection pool.
func NewPostgresStore(pool *pgxpool.Pool) (*PostgresStore, error) {
	if pool == nil {
		return nil, errors.New("feedback: postgres pool must not be nil")
	}
	return &PostgresStore{pool: pool}, nil
}

// Connect opens a pgx pool against dsn and verifies connectivity. The caller
// owns the returned pool and must Close it.
func Connect(ctx context.Context, dsn string) (*pgxpool.Pool, error) {
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		return nil, fmt.Errorf("feedback: new pool: %w", err)
	}
	pingCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if err := pool.Ping(pingCtx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("feedback: ping: %w", err)
	}
	return pool, nil
}

// Durable reports true: rows survive a restart.
func (s *PostgresStore) Durable() bool { return true }

// Record upserts one rating.
func (s *PostgresStore) Record(ctx context.Context, r Rating) error {
	clean, err := Normalize(r)
	if err != nil {
		return err
	}
	const q = `
		INSERT INTO feedback_ratings
			(org_id, user_id, run_id, skill_id, from_scope, to_scope, rating, note, created_at, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
		ON CONFLICT (org_id, run_id, user_id, skill_id) DO UPDATE SET
			from_scope = EXCLUDED.from_scope,
			to_scope   = EXCLUDED.to_scope,
			rating     = EXCLUDED.rating,
			note       = EXCLUDED.note,
			updated_at = EXCLUDED.updated_at`
	if _, err := s.pool.Exec(ctx, q,
		clean.OrgID, clean.UserID, clean.RunID, clean.SkillID,
		clean.FromScope, clean.ToScope, clean.Rating, clean.Note, clean.CreatedAt,
	); err != nil {
		return fmt.Errorf("feedback: upsert rating: %w", err)
	}
	return nil
}

// Candidates aggregates skill-attached ratings per tenant. Run-only rows
// (skill_id = '') are excluded: there is no skill to promote.
func (s *PostgresStore) Candidates(ctx context.Context, minSamples int, threshold float64) ([]Candidate, error) {
	const q = `
		SELECT org_id, skill_id, from_scope, to_scope,
		       COUNT(*) FILTER (WHERE rating = $3) AS good,
		       COUNT(*)                            AS total
		FROM feedback_ratings
		WHERE skill_id <> ''
		GROUP BY org_id, skill_id, from_scope, to_scope
		HAVING COUNT(*) >= $1
		   AND (COUNT(*) FILTER (WHERE rating = $3))::double precision / COUNT(*) >= $2`
	rows, err := s.pool.Query(ctx, q, minSamples, threshold, RatingGood)
	if err != nil {
		return nil, fmt.Errorf("feedback: query candidates: %w", err)
	}
	defer rows.Close()

	var out []Candidate
	for rows.Next() {
		var c Candidate
		var good, total int64
		if err := rows.Scan(&c.OrgID, &c.SkillID, &c.FromScope, &c.ToScope, &good, &total); err != nil {
			return nil, fmt.Errorf("feedback: scan candidate: %w", err)
		}
		if total == 0 {
			continue
		}
		c.Good = int(good)
		c.Total = int(total)
		c.Score = float64(good) / float64(total)
		out = append(out, c)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("feedback: iterate candidates: %w", err)
	}
	sortCandidates(out)
	return out, nil
}

var _ Store = (*PostgresStore)(nil)

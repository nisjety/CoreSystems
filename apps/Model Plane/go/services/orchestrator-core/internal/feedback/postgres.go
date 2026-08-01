package feedback

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/orchestrator-core/internal/quality"
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
	// The conflict target includes source and signal_kind, so an implicit signal
	// can never upsert over a human's rating — see migration 0002.
	const q = `
		INSERT INTO feedback_ratings
			(org_id, user_id, run_id, skill_id, from_scope, to_scope, rating, note,
			 source, signal_kind, signal_strength, created_at, updated_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $12)
		ON CONFLICT (org_id, run_id, user_id, skill_id, source, signal_kind) DO UPDATE SET
			from_scope      = EXCLUDED.from_scope,
			to_scope        = EXCLUDED.to_scope,
			rating          = EXCLUDED.rating,
			note            = EXCLUDED.note,
			signal_strength = EXCLUDED.signal_strength,
			updated_at      = EXCLUDED.updated_at`
	if _, err := s.pool.Exec(ctx, q,
		clean.OrgID, clean.UserID, clean.RunID, clean.SkillID,
		clean.FromScope, clean.ToScope, clean.Rating, clean.Note,
		clean.Source, clean.SignalKind, clean.SignalStrength, clean.CreatedAt,
	); err != nil {
		return fmt.Errorf("feedback: upsert rating: %w", err)
	}
	return nil
}

// Candidates aggregates skill-attached ratings per tenant. Run-only rows
// (skill_id = ”) are excluded: there is no skill to promote.
// Quarantine returns skills whose evidence has fallen below the policy's
// demotion bar, worst-first.
//
// Skill-attached rows only: quarantining acts on a skill, and a run-level rating
// (skill_id ”) names nothing to act on.
func (s *PostgresStore) Quarantine(ctx context.Context) ([]QuarantineCandidate, error) {
	const q = `
		SELECT org_id, skill_id,
		       COUNT(*) FILTER (WHERE source = 'explicit' AND rating = $1)  AS explicit_good,
		       COUNT(*) FILTER (WHERE source = 'explicit' AND rating <> $1) AS explicit_bad,
		       COALESCE(SUM(signal_strength) FILTER (WHERE source = 'implicit'), 0) AS implicit_bad
		FROM feedback_ratings
		WHERE skill_id <> ''
		GROUP BY org_id, skill_id`
	rows, err := s.pool.Query(ctx, q, RatingGood)
	if err != nil {
		return nil, fmt.Errorf("feedback: query quarantine candidates: %w", err)
	}
	defer rows.Close()

	out := make([]QuarantineCandidate, 0)
	for rows.Next() {
		var candidate QuarantineCandidate
		var good, bad int64
		var implicitBad float64
		if err := rows.Scan(
			&candidate.OrgID, &candidate.SkillID, &good, &bad, &implicitBad,
		); err != nil {
			return nil, fmt.Errorf("feedback: scan quarantine candidate: %w", err)
		}
		candidate.Score = quality.Evaluate(quality.Evidence{
			ExplicitGood:      int(good),
			ExplicitBad:       int(bad),
			ImplicitBadWeight: implicitBad,
		})
		if quality.Decide(quality.StateActive, candidate.Score) != quality.DecisionQuarantine {
			continue
		}
		out = append(out, candidate)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("feedback: iterate quarantine candidates: %w", err)
	}
	sortQuarantine(out)
	return out, nil
}

func (s *PostgresStore) Candidates(ctx context.Context, minSamples int, threshold float64) ([]Candidate, error) {
	// Aggregate here, score in Go. The Wilson bound could be written in SQL, but
	// then two stores would each carry a copy of the policy and they would drift —
	// and the drift that matters is one store promoting what the other would not.
	const q = `
		SELECT org_id, skill_id, from_scope, to_scope,
		       COUNT(*) FILTER (WHERE source = 'explicit' AND rating = $1)  AS explicit_good,
		       COUNT(*) FILTER (WHERE source = 'explicit' AND rating <> $1) AS explicit_bad,
		       COALESCE(SUM(signal_strength) FILTER (WHERE source = 'implicit'), 0) AS implicit_bad
		FROM feedback_ratings
		WHERE skill_id <> ''
		GROUP BY org_id, skill_id, from_scope, to_scope`
	rows, err := s.pool.Query(ctx, q, RatingGood)
	if err != nil {
		return nil, fmt.Errorf("feedback: query candidates: %w", err)
	}
	defer rows.Close()

	var out []Candidate
	for rows.Next() {
		var c Candidate
		var good, bad int64
		var implicitBad float64
		if err := rows.Scan(
			&c.OrgID, &c.SkillID, &c.FromScope, &c.ToScope, &good, &bad, &implicitBad,
		); err != nil {
			return nil, fmt.Errorf("feedback: scan candidate: %w", err)
		}
		score := quality.Evaluate(quality.Evidence{
			ExplicitGood:      int(good),
			ExplicitBad:       int(bad),
			ImplicitBadWeight: implicitBad,
		})
		// The caller's threshold still governs promotion — an operator's
		// configured bar is not this package's to override. What changed is WHAT
		// it is compared against: the Wilson lower bound rather than the raw
		// ratio, so a 1-of-1 skill can no longer present as perfect. The
		// consequence is deliberate and worth knowing: promotion now needs
		// volume as well as agreement, so a 9-of-10 skill is not promotable at
		// 0.8 until it has roughly thirty samples.
		if score.WeightedTotal < float64(minSamples) {
			continue
		}
		if score.LowerBound < threshold {
			continue
		}
		c.Good = int(good)
		c.Total = int(good + bad)
		c.Score = score.LowerBound
		out = append(out, c)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("feedback: iterate candidates: %w", err)
	}
	sortCandidates(out)
	return out, nil
}

var _ Store = (*PostgresStore)(nil)

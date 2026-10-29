package trust

import (
	"context"
	"fmt"
	"math"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/shared/go/orgscope"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

type Scorer struct {
	pool *pgxpool.Pool
}

func NewScorer(pool *pgxpool.Pool) *Scorer {
	return &Scorer{pool: pool}
}

var sourceAuthority = map[string]float64{
	"manual":   1.0,
	"api":      0.9,
	"quarry":   0.7,
	"crawl":    0.5,
	"external": 0.4,
}

// ScoreDocuments scores documents belonging to exactly one organization.
//
// Phase 1 RLS: orgID comes from verified caller claims (authctx) via the
// /v1/quality/trust handler, and document_ids are caller-supplied — which is
// precisely the shape the policy exists to backstop. A caller naming another
// tenant's document ids is already filtered by `org_id = $2`; under scope the
// database enforces that independently of this WHERE clause.
func (s *Scorer) ScoreDocuments(ctx context.Context, orgID string, documentIDs []string) ([]model.TrustScore, error) {
	return orgscope.InOrgScope(ctx, s.pool, orgID,
		func(ctx context.Context, tx pgx.Tx) ([]model.TrustScore, error) {
			rows, err := tx.Query(ctx, `
				SELECT document_id, title, source, updated_at
				FROM documents
				WHERE document_id = ANY($1) AND org_id = $2 AND deleted_at IS NULL
			`, documentIDs, orgID)
			if err != nil {
				return nil, fmt.Errorf("fetch documents for trust scoring: %w", err)
			}
			defer rows.Close()

			now := time.Now()
			var scores []model.TrustScore

			// Drained inside the scope: rows die at COMMIT.
			for rows.Next() {
				var docID, title, source string
				var updatedAt time.Time
				if err := rows.Scan(&docID, &title, &source, &updatedAt); err != nil {
					continue
				}

				ageDays := int(now.Sub(updatedAt).Hours() / 24)
				authority := sourceAuthority[source]
				if authority == 0 {
					authority = 0.3
				}

				freshness := freshnessScore(ageDays)
				composite := authority*0.6 + freshness*0.4

				scores = append(scores, model.TrustScore{
					DocumentID:     docID,
					Title:          title,
					Source:         source,
					AuthorityScore: authority,
					FreshnessScore: freshness,
					CompositeScore: math.Round(composite*100) / 100,
					AgeDays:        ageDays,
				})
			}

			return scores, nil
		})
}

func freshnessScore(ageDays int) float64 {
	switch {
	case ageDays <= 7:
		return 1.0
	case ageDays <= 30:
		return 0.8
	case ageDays <= 90:
		return 0.5
	case ageDays <= 365:
		return 0.3
	default:
		return 0.1
	}
}

package eval

import (
	"context"
	"encoding/json"
	"fmt"
	"math"
	"strings"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// GoldenSource loads an org's judged queries: normalized query → the ids
// (document and/or knowledge ids) a human/agent judged relevant. Evals score
// judged queries with REAL recall@10/nDCG@10/MRR; unjudged queries keep the
// candidate-count proxy, honestly labeled (QueryResult.MetricSource).
type GoldenSource interface {
	Load(ctx context.Context, orgID string) (map[string][]string, error)
}

// NormalizeQuery is the join key between judgments and traces: lowercase with
// collapsed whitespace, so cosmetic differences don't orphan a judgment.
func NormalizeQuery(q string) string {
	return strings.Join(strings.Fields(strings.ToLower(q)), " ")
}

type PostgresGoldenStore struct {
	pool *pgxpool.Pool
}

func NewPostgresGoldenStore(pool *pgxpool.Pool) *PostgresGoldenStore {
	return &PostgresGoldenStore{pool: pool}
}

// Load reads one org's judged queries.
//
// Phase 1 RLS: reached from the /v1/evals/golden handler (authctx claims) and
// from RunEval, which is always executing on behalf of one org's evaluation.
func (s *PostgresGoldenStore) Load(ctx context.Context, orgID string) (map[string][]string, error) {
	return orgscope.InOrgScope(ctx, s.pool, orgID,
		func(ctx context.Context, tx pgx.Tx) (map[string][]string, error) {
			rows, err := tx.Query(ctx, `
				SELECT query_norm, relevant_ids
				FROM eval_golden_judgments
				WHERE org_id = $1
			`, orgID)
			if err != nil {
				return nil, fmt.Errorf("query golden judgments: %w", err)
			}
			defer rows.Close()

			// Drained inside the scope: rows die at COMMIT.
			golden := make(map[string][]string)
			for rows.Next() {
				var queryNorm string
				var raw []byte
				if err := rows.Scan(&queryNorm, &raw); err != nil {
					return nil, fmt.Errorf("scan golden judgment: %w", err)
				}
				var ids []string
				if err := json.Unmarshal(raw, &ids); err != nil {
					return nil, fmt.Errorf("decode golden judgment ids: %w", err)
				}
				if len(ids) > 0 {
					golden[queryNorm] = ids
				}
			}
			if err := rows.Err(); err != nil {
				return nil, fmt.Errorf("iterate golden judgments: %w", err)
			}
			return golden, nil
		})
}

// Upsert stores/replaces the judgment for one query (org-scoped PK).
//
// Phase 1 RLS: the org_id written comes from verified caller claims, so the
// policy's WITH CHECK and this INSERT agree by construction — the scope makes
// a future edit that sourced org_id from the request body fail closed instead
// of writing into another tenant.
func (s *PostgresGoldenStore) Upsert(ctx context.Context, orgID, query string, relevantIDs []string) error {
	raw, err := json.Marshal(relevantIDs)
	if err != nil {
		return fmt.Errorf("encode golden judgment ids: %w", err)
	}
	return orgscope.WithOrgScope(ctx, s.pool, orgID, func(tx pgx.Tx) error {
		if _, err := tx.Exec(ctx, `
			INSERT INTO eval_golden_judgments (org_id, query_norm, relevant_ids)
			VALUES ($1, $2, $3)
			ON CONFLICT (org_id, query_norm)
			DO UPDATE SET relevant_ids = EXCLUDED.relevant_ids, updated_at = NOW()
		`, orgID, NormalizeQuery(query), raw); err != nil {
			return fmt.Errorf("upsert golden judgment: %w", err)
		}
		return nil
	})
}

// List returns the org's judged queries (normalized) with their relevant ids.
//
// Phase 1 RLS: no direct queries — Load opens its own scope. Deliberately not
// scoped here as well, which would nest.
func (s *PostgresGoldenStore) List(ctx context.Context, orgID string) (map[string][]string, error) {
	return s.Load(ctx, orgID)
}

// noGolden keeps the legacy two-store constructor working: every query scores
// via the labeled proxy path.
type noGolden struct{}

func (noGolden) Load(context.Context, string) (map[string][]string, error) {
	return map[string][]string{}, nil
}

// goldenMetrics computes real judged metrics over the top-10 retrieved refs.
//
//   - recall@10  = distinct judged-relevant ids found / total judged-relevant
//   - nDCG@10    = binary-gain DCG over positions / ideal DCG for min(|R|,10)
//   - MRR        = 1 / rank of the first relevant hit (0 when none)
//
// A retrieved ref matches on EITHER its document id or knowledge id, so
// judgments can name whole documents or specific chunks. DCG credits each
// relevant position (chunk-level gain); recall counts distinct judged ids.
func goldenMetrics(retrieved []RetrievedRef, relevant []string) (recall, ndcg, mrr float64) {
	if len(relevant) == 0 {
		return 0, 0, 0
	}
	rel := make(map[string]struct{}, len(relevant))
	for _, id := range relevant {
		rel[id] = struct{}{}
	}

	k := len(retrieved)
	if k > 10 {
		k = 10
	}
	found := make(map[string]struct{})
	dcg := 0.0
	for i := 0; i < k; i++ {
		_, docHit := rel[retrieved[i].DocumentID]
		_, chunkHit := rel[retrieved[i].KnowledgeID]
		if !docHit && !chunkHit {
			continue
		}
		if mrr == 0 {
			mrr = 1.0 / float64(i+1)
		}
		dcg += 1.0 / math.Log2(float64(i)+2)
		if docHit {
			found[retrieved[i].DocumentID] = struct{}{}
		}
		if chunkHit {
			found[retrieved[i].KnowledgeID] = struct{}{}
		}
	}

	idealN := len(relevant)
	if idealN > 10 {
		idealN = 10
	}
	idcg := 0.0
	for i := 0; i < idealN; i++ {
		idcg += 1.0 / math.Log2(float64(i)+2)
	}
	if idcg > 0 {
		ndcg = dcg / idcg
	}
	if ndcg > 1 {
		ndcg = 1
	}

	recall = float64(len(found)) / float64(len(relevant))
	if recall > 1 {
		recall = 1
	}
	return recall, ndcg, mrr
}

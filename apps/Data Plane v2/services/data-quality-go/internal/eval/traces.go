package eval

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/shared/go/orgscope"
)

type RetrievalTrace struct {
	TraceID    string
	Query      string
	TotalMS    int
	Candidates int
	// Retrieved carries the trace's persisted top-10 candidates (rank order)
	// so golden-set judgments can compute real recall/nDCG/MRR.
	Retrieved []RetrievedRef
}

// RetrievedRef is one ranked retrieval hit; either id may be empty (wiki
// candidates have no knowledge_unit row, some legacy traces predate columns).
type RetrievedRef struct {
	KnowledgeID string
	DocumentID  string
}

type TraceSource interface {
	Recent(context.Context, string, int) ([]RetrievalTrace, error)
}

type PostgresTraceSource struct {
	pool *pgxpool.Pool
}

func NewPostgresTraceSource(pool *pgxpool.Pool) *PostgresTraceSource {
	return &PostgresTraceSource{pool: pool}
}

// Recent loads one org's recent retrieval traces and their top-10 candidates.
//
// Phase 1 RLS: orgID is a single verified organization on both paths that
// reach here — the /v1/evals request handler (authctx claims) and the durable
// recovery loop, which calls RunEval once per recovered run with that row's
// own org_id. Both queries share one scoped transaction: the candidate lookup
// keys off trace ids read by the first query, so a single snapshot keeps them
// from disagreeing, and retrieval_candidates carries no org_id of its own —
// the child policy from 20260809180000_org_rls_child_tables.sql derives one
// from its retrieval_runs parent, which is what makes the otherwise
// org-unfiltered `trace_id = ANY($1)` lookup tenant-safe rather than merely
// tenant-correct-by-construction.
func (s *PostgresTraceSource) Recent(ctx context.Context, orgID string, limit int) ([]RetrievalTrace, error) {
	return orgscope.InOrgScope(ctx, s.pool, orgID,
		func(ctx context.Context, tx pgx.Tx) ([]RetrievalTrace, error) {
			rows, err := tx.Query(ctx, `
				SELECT trace_id, query, total_ms, candidate_count_reranked
				FROM retrieval_runs
				WHERE org_id = $1
				ORDER BY created_at DESC
				LIMIT $2
			`, orgID, limit)
			if err != nil {
				return nil, fmt.Errorf("query retrieval traces: %w", err)
			}
			defer rows.Close()

			// Drained before the next query: the scope requires every pgx.Rows
			// to be consumed inside the callback, and pgx forbids a second
			// query on the same transaction while these rows are open.
			traces := make([]RetrievalTrace, 0, limit)
			traceIDs := make([]string, 0, limit)
			for rows.Next() {
				var trace RetrievalTrace
				if err := rows.Scan(&trace.TraceID, &trace.Query, &trace.TotalMS, &trace.Candidates); err != nil {
					return nil, fmt.Errorf("scan retrieval trace: %w", err)
				}
				traces = append(traces, trace)
				traceIDs = append(traceIDs, trace.TraceID)
			}
			if err := rows.Err(); err != nil {
				return nil, fmt.Errorf("iterate retrieval traces: %w", err)
			}
			rows.Close()
			if len(traceIDs) == 0 {
				return traces, nil
			}

			// One batch query for the traces' persisted top-10 candidates — the ids
			// golden-set judgments score against. Ordered so append preserves rank.
			// retrieval-engine persists rank 1-based (rank+1), so the top-10 occupy
			// ranks 1..10 — `rank <= 10`, not `< 10` (which would drop rank 10).
			candRows, err := tx.Query(ctx, `
				SELECT trace_id, knowledge_id, document_id
				FROM retrieval_candidates
				WHERE trace_id = ANY($1) AND rank <= 10
				ORDER BY trace_id, rank
			`, traceIDs)
			if err != nil {
				return nil, fmt.Errorf("query retrieval candidates: %w", err)
			}
			defer candRows.Close()

			byTrace := make(map[string][]RetrievedRef, len(traceIDs))
			for candRows.Next() {
				var traceID string
				var knowledgeID, documentID *string
				if err := candRows.Scan(&traceID, &knowledgeID, &documentID); err != nil {
					return nil, fmt.Errorf("scan retrieval candidate: %w", err)
				}
				ref := RetrievedRef{}
				if knowledgeID != nil {
					ref.KnowledgeID = *knowledgeID
				}
				if documentID != nil {
					ref.DocumentID = *documentID
				}
				byTrace[traceID] = append(byTrace[traceID], ref)
			}
			if err := candRows.Err(); err != nil {
				return nil, fmt.Errorf("iterate retrieval candidates: %w", err)
			}
			for i := range traces {
				traces[i].Retrieved = byTrace[traces[i].TraceID]
			}
			return traces, nil
		})
}

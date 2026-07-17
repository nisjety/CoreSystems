package eval

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5/pgxpool"
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

func (s *PostgresTraceSource) Recent(ctx context.Context, orgID string, limit int) ([]RetrievalTrace, error) {
	rows, err := s.pool.Query(ctx, `
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
	if len(traceIDs) == 0 {
		return traces, nil
	}

	// One batch query for the traces' persisted top-10 candidates — the ids
	// golden-set judgments score against. Ordered so append preserves rank.
	candRows, err := s.pool.Query(ctx, `
		SELECT trace_id, knowledge_id, document_id
		FROM retrieval_candidates
		WHERE trace_id = ANY($1) AND rank < 10
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
}

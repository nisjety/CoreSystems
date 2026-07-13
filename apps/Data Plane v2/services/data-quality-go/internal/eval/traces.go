package eval

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5/pgxpool"
)

type RetrievalTrace struct {
	Query      string
	TotalMS    int
	Candidates int
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
		SELECT query, total_ms, candidate_count_reranked
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
	for rows.Next() {
		var trace RetrievalTrace
		if err := rows.Scan(&trace.Query, &trace.TotalMS, &trace.Candidates); err != nil {
			return nil, fmt.Errorf("scan retrieval trace: %w", err)
		}
		traces = append(traces, trace)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate retrieval traces: %w", err)
	}
	return traces, nil
}

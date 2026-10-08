package postgres

import (
	"context"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

func (s *Store) UsageAnalytics(ctx context.Context, f ledger.AggregateFilter) ([]ledger.UsageBucket, error) {
	where, args := buildWhere(f)
	rows, err := s.pool.Query(ctx, `SELECT
 to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD'), COALESCE(model,''),
 COALESCE(provider_used,''), COALESCE(cost_basis,'unknown'), COUNT(*),
 COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
 COALESCE(SUM(cost_usd) FILTER (WHERE cost_basis='metered'),0)::double precision,
 COUNT(*) FILTER (WHERE input_tokens=0 AND output_tokens=0)
 FROM cost_entries`+where+` GROUP BY 1,2,3,4 ORDER BY 1,2,3,4 LIMIT 10001`, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []ledger.UsageBucket{}
	for rows.Next() {
		var b ledger.UsageBucket
		if err := rows.Scan(&b.Day, &b.Model, &b.Provider, &b.Basis, &b.Requests, &b.InputTokens, &b.OutputTokens, &b.MeteredCostUSD, &b.UnmeasuredRequests); err != nil {
			return nil, err
		}
		out = append(out, b)
		if len(out) > ledger.MaxAnalyticsBuckets {
			return nil, ledger.ErrAnalyticsTooLarge
		}
	}
	return out, rows.Err()
}

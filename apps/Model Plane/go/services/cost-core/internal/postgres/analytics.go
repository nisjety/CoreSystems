package postgres

import (
	"context"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// notSubscription keeps a subscription turn out of the cache figures: it has no
// cache spend to report. A NULL basis is an older producer and stays in.
const notSubscription = `cost_basis IS DISTINCT FROM 'subscription'`

func (s *Store) UsageAnalytics(ctx context.Context, f ledger.AggregateFilter) ([]ledger.UsageBucket, error) {
	where, args := buildWhere(f)
	rows, err := s.pool.Query(ctx, `SELECT
 to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD'), COALESCE(model,''),
 COALESCE(provider_used,''), COALESCE(cost_basis,'unknown'), COUNT(*),
 COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
 COALESCE(SUM(cost_usd) FILTER (WHERE cost_basis='metered'),0)::double precision,
 COUNT(*) FILTER (WHERE input_tokens=0 AND output_tokens=0),
 COALESCE(SUM(cache_read_input_tokens) FILTER (WHERE `+notSubscription+`),0),
 COALESCE(SUM(cache_creation_input_tokens) FILTER (WHERE `+notSubscription+`),0),
 COALESCE(SUM(input_tokens) FILTER (WHERE `+notSubscription+` AND (cache_read_input_tokens>0
  OR cache_creation_input_tokens>0 OR COALESCE(cache_miss_reason,'') NOT IN ('','below_minimum'))),0)
 FROM cost_entries`+where+` GROUP BY 1,2,3,4 ORDER BY 1,2,3,4 LIMIT 10001`, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []ledger.UsageBucket{}
	for rows.Next() {
		var b ledger.UsageBucket
		if err := rows.Scan(&b.Day, &b.Model, &b.Provider, &b.Basis, &b.Requests, &b.InputTokens, &b.OutputTokens, &b.MeteredCostUSD, &b.UnmeasuredRequests,
			&b.CacheReadInputTokens, &b.CacheCreationInputTokens, &b.CacheReportedInputTokens); err != nil {
			return nil, err
		}
		out = append(out, b)
		if len(out) > ledger.MaxAnalyticsBuckets {
			return nil, ledger.ErrAnalyticsTooLarge
		}
	}
	return out, rows.Err()
}

// CacheMissAnalytics counts classified cache misses by reason. Entries with no
// reason (a hit, an unclassified or older producer) are not rows here.
func (s *Store) CacheMissAnalytics(ctx context.Context, f ledger.AggregateFilter) ([]ledger.CacheMissBucket, error) {
	where, args := buildWhere(f)
	if where == "" {
		where = " WHERE "
	} else {
		where += " AND "
	}
	rows, err := s.pool.Query(ctx, `SELECT
 to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD'), COALESCE(model,''),
 COALESCE(provider_used,''), cache_miss_reason, COUNT(*)
 FROM cost_entries`+where+`cache_miss_reason IS NOT NULL AND `+notSubscription+`
 GROUP BY 1,2,3,4 ORDER BY 1,2,3,4 LIMIT 10001`, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []ledger.CacheMissBucket{}
	for rows.Next() {
		var b ledger.CacheMissBucket
		if err := rows.Scan(&b.Day, &b.Model, &b.Provider, &b.Reason, &b.Requests); err != nil {
			return nil, err
		}
		out = append(out, b)
		if len(out) > ledger.MaxAnalyticsBuckets {
			return nil, ledger.ErrAnalyticsTooLarge
		}
	}
	return out, rows.Err()
}

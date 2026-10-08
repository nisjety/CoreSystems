package postgres

import (
	"context"
	"fmt"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// No LIMIT is applied to raw entries: each tenant's entire window contributes
// to its users. One grouped query also keeps all users on one DB snapshot.
func (s *Store) AggregateUsers(ctx context.Context, f ledger.AggregateFilter) ([]ledger.UserUsage, error) {
	where, args := buildWhere(f)
	rows, err := s.pool.Query(ctx, `SELECT COALESCE(user_id,''), COUNT(*),
	 COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
	 COALESCE(SUM(cost_usd) FILTER (WHERE cost_basis = 'metered'),0)::double precision,
	 COUNT(*) FILTER (WHERE cost_basis = 'metered'),
	 COUNT(*) FILTER (WHERE cost_basis = 'subscription'),
	 COUNT(*) FILTER (WHERE cost_basis IS NULL),
	 COUNT(*) FILTER (WHERE input_tokens = 0 AND output_tokens = 0)
	 FROM cost_entries`+where+` GROUP BY COALESCE(user_id,'') ORDER BY COALESCE(user_id,'')`, args...)
	if err != nil {
		return nil, fmt.Errorf("user usage: %w", err)
	}
	defer rows.Close()
	out := []ledger.UserUsage{}
	for rows.Next() {
		var u ledger.UserUsage
		if err := rows.Scan(&u.UserID, &u.EntryCount, &u.InputTokens, &u.OutputTokens, &u.MeteredCostUSD, &u.MeteredEntries,
			&u.SubscriptionEntries, &u.UnknownBasisEntries, &u.UnmeasuredTokenEntries); err != nil {
			return nil, err
		}
		out = append(out, u)
	}
	return out, rows.Err()
}

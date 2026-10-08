package postgres

import (
	"context"
	"time"
)

func (s *Store) ReportingStart(ctx context.Context, orgID string) (*time.Time, error) {
	var start *time.Time
	err := s.pool.QueryRow(ctx, `SELECT MAX(started_at) FROM usage_reporting_periods WHERE org_id = $1`, orgID).Scan(&start)
	return start, err
}

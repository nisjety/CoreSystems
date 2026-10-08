package server

import (
	"context"
	"time"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// Called only after identity is pinned. Billing, budgets and raw ledger reads
// deliberately do not apply this display-only reporting period.
func (s *Server) reportingFilter(ctx context.Context, f ledger.AggregateFilter) (ledger.AggregateFilter, *time.Time, error) {
	reader, ok := s.ledger.(ledger.ReportingStartReader)
	if !ok || f.OrgID == "" {
		return f, nil, nil
	}
	start, err := reader.ReportingStart(ctx, f.OrgID)
	if err != nil {
		return f, nil, err
	}
	if start != nil && f.Since.Before(*start) {
		f.Since = *start
	}
	return f, start, nil
}

package insights

import (
	"context"
	"encoding/json"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// PGRepository is the durable (Postgres) metric-event store for W3. The connector
// registry is static/in-code (org-independent), so ListConnectorSlots returns the
// same registry the in-memory repo does — only per-org metric events are persisted.
type PGRepository struct {
	pool       *pgxpool.Pool
	connectors []ConnectorSlot
}

func NewPGRepository(pool *pgxpool.Pool, connectors []ConnectorSlot) *PGRepository {
	return &PGRepository{pool: pool, connectors: copyConnectorSlots(connectors)}
}

// RecordMetricEvent persists a metric event idempotently by id (ON CONFLICT DO
// NOTHING) so duplicate JetStream deliveries never double-count.
func (r *PGRepository) RecordMetricEvent(ctx context.Context, event MetricEvent) (*MetricEvent, error) {
	dims, err := json.Marshal(copyMap(event.Dimensions))
	if err != nil {
		return nil, err
	}
	if _, err := r.pool.Exec(ctx, `
INSERT INTO insight_metric_events
	(id, org_id, surface, metric, value, unit, source, connector_type, dimensions, occurred_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::jsonb, $10)
ON CONFLICT (id) DO NOTHING`,
		event.ID, event.OrgID, event.Surface, event.Metric, event.Value, event.Unit,
		event.Source, event.ConnectorType, dims, event.OccurredAt); err != nil {
		return nil, err
	}
	stored := copyMetricEvent(event)
	return &stored, nil
}

// ListMetricEvents returns org-scoped events for the requested surfaces within an
// optional time window. The service normalizes query.Surfaces (defaulting to all
// supported surfaces), matching the in-memory repo's surface filter.
func (r *PGRepository) ListMetricEvents(ctx context.Context, query OverviewQuery) ([]MetricEvent, error) {
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, surface, metric, value, unit, source, connector_type, dimensions, occurred_at
FROM insight_metric_events
WHERE org_id = $1
  AND surface = ANY($2)
  AND ($3::timestamptz IS NULL OR occurred_at >= $3)
  AND ($4::timestamptz IS NULL OR occurred_at <= $4)
ORDER BY occurred_at DESC`,
		query.OrgID, query.Surfaces, query.From, query.To)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	events := []MetricEvent{}
	for rows.Next() {
		var e MetricEvent
		var dims []byte
		if err := rows.Scan(
			&e.ID, &e.OrgID, &e.Surface, &e.Metric, &e.Value, &e.Unit,
			&e.Source, &e.ConnectorType, &dims, &e.OccurredAt,
		); err != nil {
			return nil, err
		}
		if len(dims) > 0 {
			_ = json.Unmarshal(dims, &e.Dimensions)
		}
		if e.Dimensions == nil {
			e.Dimensions = map[string]any{}
		}
		events = append(events, e)
	}
	return events, rows.Err()
}

// ListConnectorSlots returns the static connector registry (org-independent).
func (r *PGRepository) ListConnectorSlots(_ context.Context, _ string) ([]ConnectorSlot, error) {
	return copyConnectorSlots(r.connectors), nil
}

// ListOrgIDsWithMetricsSince returns the distinct org_ids that recorded at least
// one metric event at or after `since`, ordered for deterministic iteration.
func (r *PGRepository) ListOrgIDsWithMetricsSince(ctx context.Context, since time.Time) ([]string, error) {
	rows, err := r.pool.Query(ctx, `
SELECT DISTINCT org_id
FROM insight_metric_events
WHERE occurred_at >= $1
ORDER BY org_id`, since)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	orgIDs := []string{}
	for rows.Next() {
		var orgID string
		if err := rows.Scan(&orgID); err != nil {
			return nil, err
		}
		orgIDs = append(orgIDs, orgID)
	}
	return orgIDs, rows.Err()
}

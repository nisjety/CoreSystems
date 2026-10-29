package social

import (
	"context"
	"encoding/json"
	"strconv"
	"strings"
)

func (r *PGRepository) ListAccountOrgIDs(ctx context.Context) ([]string, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	rows, err := r.pool.Query(ctx, `
SELECT DISTINCT org_id
FROM social_accounts
ORDER BY org_id ASC`)
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

// UpsertProviderMetrics persists snapshot rows idempotently: the conflict
// target matches social_provider_metrics_dedup_unique, so re-running a
// snapshot for the same day updates values in place.
func (r *PGRepository) UpsertProviderMetrics(ctx context.Context, metrics []ProviderMetric) (int, error) {
	if err := r.ensureConfigured(); err != nil {
		return 0, err
	}
	if len(metrics) == 0 {
		return 0, nil
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return 0, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	persisted := 0
	for _, metric := range metrics {
		metric.OrgID = strings.TrimSpace(metric.OrgID)
		metric.AccountID = strings.TrimSpace(metric.AccountID)
		metric.ConnectionID = strings.TrimSpace(metric.ConnectionID)
		metric.ProviderKey = normalizePlatform(metric.ProviderKey)
		metric.MetricName = strings.TrimSpace(metric.MetricName)
		metric.Dimensions = ensureMap(metric.Dimensions)
		if metric.OrgID == "" || metric.AccountID == "" || metric.MetricName == "" || metric.SnapshotDate.IsZero() {
			continue
		}
		if _, err := tx.Exec(ctx, `
INSERT INTO social_provider_metrics (
	id, org_id, account_id, connection_id, provider_key, metric_name,
	metric_value, dimensions, snapshot_date
) VALUES (
	$1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9
)
ON CONFLICT (org_id, account_id, metric_name, dimensions, snapshot_date)
DO UPDATE SET
	metric_value = EXCLUDED.metric_value,
	connection_id = EXCLUDED.connection_id,
	provider_key = EXCLUDED.provider_key`,
			newID("socmet"), metric.OrgID, metric.AccountID, metric.ConnectionID, metric.ProviderKey,
			metric.MetricName, metric.MetricValue, mustJSON(metric.Dimensions), metric.SnapshotDate); err != nil {
			return 0, err
		}
		persisted++
	}

	if err := tx.Commit(ctx); err != nil {
		return 0, err
	}
	committed = true
	return persisted, nil
}

// ListProviderMetrics reads persisted snapshot rows for an org, optionally
// narrowed to one account and/or one snapshot date. This is the read side of
// UpsertProviderMetrics — added for insight-core's provider-metrics
// subscriber (task #28), which fetches the real values behind the
// metrics.snapshotted lifecycle event (that event carries only a summary
// count, by design, to keep NATS payloads small).
func (r *PGRepository) ListProviderMetrics(ctx context.Context, filter ProviderMetricsFilter) ([]ProviderMetric, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	orgID := strings.TrimSpace(filter.OrgID)
	if orgID == "" {
		return nil, nil
	}
	query := `
SELECT org_id, account_id, connection_id, provider_key, metric_name, metric_value, dimensions, snapshot_date
FROM social_provider_metrics
WHERE org_id = $1`
	args := []any{orgID}
	if accountID := strings.TrimSpace(filter.AccountID); accountID != "" {
		args = append(args, accountID)
		query += " AND account_id = $" + strconv.Itoa(len(args))
	}
	if !filter.SnapshotDate.IsZero() {
		args = append(args, filter.SnapshotDate)
		query += " AND snapshot_date = $" + strconv.Itoa(len(args))
	}
	query += " ORDER BY metric_name ASC"

	rows, err := r.pool.Query(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	metrics := []ProviderMetric{}
	for rows.Next() {
		var (
			metric        ProviderMetric
			dimensionsRaw []byte
		)
		if err := rows.Scan(
			&metric.OrgID, &metric.AccountID, &metric.ConnectionID, &metric.ProviderKey,
			&metric.MetricName, &metric.MetricValue, &dimensionsRaw, &metric.SnapshotDate,
		); err != nil {
			return nil, err
		}
		if len(dimensionsRaw) > 0 {
			_ = json.Unmarshal(dimensionsRaw, &metric.Dimensions)
		}
		metrics = append(metrics, metric)
	}
	return metrics, rows.Err()
}

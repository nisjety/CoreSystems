// Package registry — Postgres-backed capability registry source.
//
// CapabilitiesSource implements the Source interface by reading from the
// `capabilities` table added in migration 0003.  It supplements the static
// seed so that the registry can serve both hard-coded bootstrap capabilities
// and operator-managed ones without a service restart.
package registry

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// CapabilitiesSource loads Capability rows from the `capabilities` table.
// It is intentionally simple — the full CRUD and audit lives in
// CapabilitiesStore; this type only satisfies Source for use in a Registry.
type CapabilitiesSource struct {
	pool  *pgxpool.Pool
	orgID string // empty string = all orgs
}

// NewCapabilitiesSource constructs a CapabilitiesSource.  Pass an empty
// orgID to load capabilities for every org (global registry mode); pass a
// specific orgID to scope to a single tenant.
func NewCapabilitiesSource(pool *pgxpool.Pool, orgID string) (*CapabilitiesSource, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool is required")
	}
	return &CapabilitiesSource{pool: pool, orgID: orgID}, nil
}

// Load implements Source by querying the capabilities table.
func (s *CapabilitiesSource) Load() ([]*models.Capability, error) {
	ctx := context.Background()

	var (
		rows pgx.Rows
		err  error
	)

	if s.orgID == "" {
		rows, err = s.pool.Query(ctx, `
			SELECT id, org_id, kind, name, version, description,
			       risk_level, scope, lazy_load, enabled,
			       idempotency_key, enabled_for_scopes, rollout_state,
			       availability_state, availability_reason_code,
			       availability_reason, execution_mode, cost_class,
			       health_checked_at
			FROM capabilities
			WHERE deleted_at IS NULL
			ORDER BY kind, name
		`)
	} else {
		rows, err = s.pool.Query(ctx, `
			SELECT id, org_id, kind, name, version, description,
			       risk_level, scope, lazy_load, enabled,
			       idempotency_key, enabled_for_scopes, rollout_state,
			       availability_state, availability_reason_code,
			       availability_reason, execution_mode, cost_class,
			       health_checked_at
			FROM capabilities
			WHERE deleted_at IS NULL
			  AND (org_id = $1 OR org_id = 'global')
			ORDER BY kind, name
		`, s.orgID)
	}
	if err != nil {
		return nil, fmt.Errorf("capabilities source query: %w", err)
	}
	defer rows.Close()

	var caps []*models.Capability
	for rows.Next() {
		c := &models.Capability{}
		var scopes []string
		if err := rows.Scan(
			&c.ID, &c.OrgID, &c.Kind, &c.Name, &c.Version,
			&c.Description, &c.RiskLevel, &c.Scope,
			&c.LazyLoad, &c.Enabled, &c.IdempotencyKey, &scopes, &c.RolloutState,
			&c.AvailabilityState, &c.ReasonCode, &c.Reason,
			&c.ExecutionMode, &c.CostClass, &c.HealthCheckedAt,
		); err != nil {
			return nil, fmt.Errorf("capabilities source scan: %w", err)
		}
		c.EnabledForScopes = scopes
		caps = append(caps, c)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("capabilities source rows: %w", err)
	}
	return caps, nil
}

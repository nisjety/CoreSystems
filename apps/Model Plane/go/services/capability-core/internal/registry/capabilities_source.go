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
	db    capabilitiesQuerier
	orgID string // empty string = all orgs
}

// capabilitiesQuerier is the one read the source makes. *pgxpool.Pool satisfies
// it; a unit test substitutes rows without a database.
type capabilitiesQuerier interface {
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
}

// NewCapabilitiesSource constructs a CapabilitiesSource.  Pass an empty
// orgID to load capabilities for every org (global registry mode); pass a
// specific orgID to scope to a single tenant.
func NewCapabilitiesSource(pool *pgxpool.Pool, orgID string) (*CapabilitiesSource, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool is required")
	}
	return &CapabilitiesSource{db: pool, orgID: orgID}, nil
}

// sourceCapabilityColumns is the column list both Load queries select, in the
// order scanSourceCapability scans it. The class and floor are read here so the
// in-memory registry (and the gRPC read surface built on it) reports the stored
// declaration instead of an empty value, which reads as the strictest.
const sourceCapabilityColumns = `id, org_id, kind, name, version, description,
			       risk_level, scope, lazy_load, enabled,
			       idempotency_key, enabled_for_scopes, rollout_state,
			       availability_state, availability_reason_code,
			       availability_reason, execution_mode, cost_class,
			       health_checked_at, side_effect_class, approval_floor`

// scanSourceCapability scans one row of sourceCapabilityColumns.
func scanSourceCapability(row pgxScanner) (*models.Capability, error) {
	c := &models.Capability{}
	var scopes []string
	if err := row.Scan(
		&c.ID, &c.OrgID, &c.Kind, &c.Name, &c.Version,
		&c.Description, &c.RiskLevel, &c.Scope,
		&c.LazyLoad, &c.Enabled, &c.IdempotencyKey, &scopes, &c.RolloutState,
		&c.AvailabilityState, &c.ReasonCode, &c.Reason,
		&c.ExecutionMode, &c.CostClass, &c.HealthCheckedAt,
		&c.SideEffectClass, &c.ApprovalFloor,
	); err != nil {
		return nil, err
	}
	c.EnabledForScopes = scopes
	return c, nil
}

// Load implements Source by querying the capabilities table.
func (s *CapabilitiesSource) Load() ([]*models.Capability, error) {
	ctx := context.Background()

	var (
		rows pgx.Rows
		err  error
	)

	if s.orgID == "" {
		rows, err = s.db.Query(ctx, "SELECT "+sourceCapabilityColumns+`
			FROM capabilities
			WHERE deleted_at IS NULL
			ORDER BY kind, name
		`)
	} else {
		rows, err = s.db.Query(ctx, "SELECT "+sourceCapabilityColumns+`
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
		c, err := scanSourceCapability(rows)
		if err != nil {
			return nil, fmt.Errorf("capabilities source scan: %w", err)
		}
		caps = append(caps, c)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("capabilities source rows: %w", err)
	}
	return caps, nil
}

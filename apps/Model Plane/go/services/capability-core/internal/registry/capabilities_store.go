// Package registry — CapabilitiesStore: full CRUD for the capabilities table.
//
// Separate from CapabilitiesSource (read-only, implements Source) so callers
// that only need to query the registry don't take a write-capable handle.
package registry

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/scoring"
)

// ErrRiskFloorViolation is returned by Upsert when a write would lower a
// floored capability's risk_level (one whose seed or currently persisted
// risk_level is high) without the caller holding authz.RiskOverrideScope.
// See POL-1 in apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md.
var ErrRiskFloorViolation = errors.New("capability risk floor: capability:risk:override is required to lower a high-risk capability's risk_level")

// CapabilityRow is the full mutable row for the capabilities table.
type CapabilityRow struct {
	ID                string
	OrgID             string
	Kind              string
	Name              string
	Version           string
	Description       string
	RiskLevel         string
	Scope             string
	LazyLoad          bool
	Enabled           bool
	IdempotencyKey    string
	SchemaInput       []byte
	SchemaOutput      []byte
	ConfigJSON        []byte
	Tags              []string
	EnabledForScopes  []string
	SuccessRate       float64
	SchemaFailRate    float64
	P95LatencyMS      float64
	MeanCostUSD       float64
	ApprovalRate      float64
	IncidentCount     int
	OperatorRating    float64
	RolloutState      string
	AvailabilityState string
	ReasonCode        string
	Reason            string
	ExecutionMode     string
	CostClass         string
	HealthCheckedAt   *time.Time
	CreatedBy         string
	CreatedAt         time.Time
	UpdatedAt         time.Time
}

// CapabilitiesStore provides CRUD access to the capabilities table.
type CapabilitiesStore struct {
	pool capabilitiesDatabase
}

type capabilitiesDatabase interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
}

// AvailabilityUpdate is a normalized runtime health attestation. Callers must
// derive it from authenticated workload input and registry risk policy.
type AvailabilityUpdate struct {
	ExpectedVersion string
	State           string
	ReasonCode      string
	Reason          string
	ExecutionMode   string
	CostClass       string
	HealthCheckedAt time.Time
}

// NewCapabilitiesStore constructs a store.
func NewCapabilitiesStore(pool *pgxpool.Pool) (*CapabilitiesStore, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool required")
	}
	return &CapabilitiesStore{pool: pool}, nil
}

// Upsert inserts or updates a capability row using ON CONFLICT on
// (org_id,kind,name). hasRiskOverride must be true for a caller authorized
// (via authz.RiskOverrideScope) to lower the risk_level of a floored
// capability; ordinary capability:write callers must pass false. See
// enforceRiskFloor and POL-1 in apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md.
func (s *CapabilitiesStore) Upsert(ctx context.Context, r *CapabilityRow, hasRiskOverride bool) error {
	if r == nil {
		return fmt.Errorf("capability is required")
	}
	if !models.IsSupportedRiskLevel(r.RiskLevel) {
		return fmt.Errorf("unsupported capability risk level %q", r.RiskLevel)
	}
	if err := s.enforceRiskFloor(ctx, r, hasRiskOverride); err != nil {
		return err
	}
	if r.SchemaInput == nil {
		r.SchemaInput = []byte("{}")
	}
	if r.SchemaOutput == nil {
		r.SchemaOutput = []byte("{}")
	}
	if r.ConfigJSON == nil {
		r.ConfigJSON = []byte("{}")
	}
	// The tags / enabled_for_scopes columns are NOT NULL DEFAULT '{}'; a nil
	// Go slice encodes as SQL NULL and violates that constraint, so coalesce to
	// an empty slice.
	if r.Tags == nil {
		r.Tags = []string{}
	}
	if r.EnabledForScopes == nil {
		r.EnabledForScopes = []string{}
	}
	if r.RolloutState == "" {
		r.RolloutState = "stable"
	}
	now := time.Now().UTC()
	_, err := s.pool.Exec(ctx, `
		INSERT INTO capabilities (
			id, org_id, kind, name, version, description,
			risk_level, scope, lazy_load, enabled, idempotency_key,
			schema_input, schema_output, config_json, tags, enabled_for_scopes,
			rollout_state, created_by, created_at, updated_at
		) VALUES (
			$1,$2,$3,$4,$5,$6,
			$7,$8,$9,$10,$11,
			$12,$13,$14,$15,$16,
			$17,$18,$19,$20
		)
		ON CONFLICT (org_id, kind, name) WHERE deleted_at IS NULL DO UPDATE SET
			version          = EXCLUDED.version,
			description      = EXCLUDED.description,
			risk_level       = EXCLUDED.risk_level,
			scope            = EXCLUDED.scope,
			lazy_load        = EXCLUDED.lazy_load,
			enabled          = EXCLUDED.enabled,
			schema_input     = EXCLUDED.schema_input,
			schema_output    = EXCLUDED.schema_output,
			config_json      = EXCLUDED.config_json,
			tags             = EXCLUDED.tags,
			enabled_for_scopes = EXCLUDED.enabled_for_scopes,
			rollout_state    = EXCLUDED.rollout_state,
			availability_state = CASE
				WHEN (capabilities.version, capabilities.risk_level, capabilities.schema_input, capabilities.schema_output, capabilities.config_json)
				  IS DISTINCT FROM (EXCLUDED.version, EXCLUDED.risk_level, EXCLUDED.schema_input, EXCLUDED.schema_output, EXCLUDED.config_json)
				THEN 'unavailable' ELSE capabilities.availability_state END,
			availability_reason_code = CASE
				WHEN (capabilities.version, capabilities.risk_level, capabilities.schema_input, capabilities.schema_output, capabilities.config_json)
				  IS DISTINCT FROM (EXCLUDED.version, EXCLUDED.risk_level, EXCLUDED.schema_input, EXCLUDED.schema_output, EXCLUDED.config_json)
				THEN 'health_not_attested' ELSE capabilities.availability_reason_code END,
			availability_reason = CASE
				WHEN (capabilities.version, capabilities.risk_level, capabilities.schema_input, capabilities.schema_output, capabilities.config_json)
				  IS DISTINCT FROM (EXCLUDED.version, EXCLUDED.risk_level, EXCLUDED.schema_input, EXCLUDED.schema_output, EXCLUDED.config_json)
				THEN 'Capability changed and requires a new health attestation.' ELSE capabilities.availability_reason END,
			execution_mode = CASE
				WHEN (capabilities.version, capabilities.risk_level, capabilities.schema_input, capabilities.schema_output, capabilities.config_json)
				  IS DISTINCT FROM (EXCLUDED.version, EXCLUDED.risk_level, EXCLUDED.schema_input, EXCLUDED.schema_output, EXCLUDED.config_json)
				THEN 'unavailable' ELSE capabilities.execution_mode END,
			cost_class = CASE
				WHEN (capabilities.version, capabilities.risk_level, capabilities.schema_input, capabilities.schema_output, capabilities.config_json)
				  IS DISTINCT FROM (EXCLUDED.version, EXCLUDED.risk_level, EXCLUDED.schema_input, EXCLUDED.schema_output, EXCLUDED.config_json)
				THEN 'unknown' ELSE capabilities.cost_class END,
			health_checked_at = CASE
				WHEN (capabilities.version, capabilities.risk_level, capabilities.schema_input, capabilities.schema_output, capabilities.config_json)
				  IS DISTINCT FROM (EXCLUDED.version, EXCLUDED.risk_level, EXCLUDED.schema_input, EXCLUDED.schema_output, EXCLUDED.config_json)
				THEN NULL ELSE capabilities.health_checked_at END,
			updated_at       = EXCLUDED.updated_at
	`,
		r.ID, r.OrgID, r.Kind, r.Name, r.Version, r.Description,
		r.RiskLevel, r.Scope, r.LazyLoad, r.Enabled, r.IdempotencyKey,
		r.SchemaInput, r.SchemaOutput, r.ConfigJSON, r.Tags, r.EnabledForScopes,
		r.RolloutState, r.CreatedBy, now, now,
	)
	return err
}

// enforceRiskFloor blocks a write from lowering a capability's risk_level
// below high once it is high, unless hasRiskOverride is true. It does NOT
// otherwise restrict risk_level changes — a medium or low capability may
// freely move between medium and low under plain capability:write. Only the
// high floor is protected, matching POL-1's finding that no seeded High-risk
// capability (cap.command.shell, cap.browser.open, ...) may be silently
// downgraded to disable its human-approval gate in policy/engine.go.
//
// A capability is "floored" when either:
//   - its currently persisted risk_level, read fresh and matched on the same
//     natural key (org_id, kind, name) this upsert itself targets, is high; or
//   - no current row can be read for it, but its id is one of the statically
//     seeded RiskHigh capabilities (models.IsSeededHighRiskCapability) — the
//     fail-closed backstop: refuse rather than assume a missing or unreadable
//     protected row is safe to write at a lower level.
//
// Known limitation: the read here and the upsert's write are two round trips,
// not one transaction, so a precisely-timed concurrent write could still race
// past this check. That residual is disclosed in docs/SECURITY.md; it is not
// the silent, always-open gap this floor closes.
func (s *CapabilitiesStore) enforceRiskFloor(ctx context.Context, r *CapabilityRow, hasRiskOverride bool) error {
	if hasRiskOverride || r.RiskLevel == models.RiskHigh {
		return nil // Upgrades to high, and any override-scoped write, always pass.
	}

	var priorRiskLevel string
	err := s.pool.QueryRow(ctx, `
		SELECT risk_level FROM capabilities
		WHERE org_id = $1 AND kind = $2 AND name = $3 AND deleted_at IS NULL
	`, r.OrgID, r.Kind, r.Name).Scan(&priorRiskLevel)

	switch {
	case err == nil:
		if priorRiskLevel == models.RiskHigh {
			return fmt.Errorf("%w: %s is currently high-risk and cannot be lowered to %q", ErrRiskFloorViolation, r.ID, r.RiskLevel)
		}
		return nil
	case errors.Is(err, pgx.ErrNoRows):
		if models.IsSeededHighRiskCapability(r.ID) {
			return fmt.Errorf("%w: %s is a protected high-risk capability with no readable prior state", ErrRiskFloorViolation, r.ID)
		}
		return nil // Genuinely new, unfloored capability: nothing to downgrade from.
	default:
		return fmt.Errorf("capability risk floor check for %s: %w", r.ID, err)
	}
}

// Get returns a single capability by ID.
func (s *CapabilitiesStore) Get(ctx context.Context, id string) (*CapabilityRow, error) {
	row := s.pool.QueryRow(ctx, `
		SELECT id, org_id, kind, name, version, description,
		       risk_level, scope, lazy_load, enabled, idempotency_key,
		       schema_input, schema_output, config_json, tags, enabled_for_scopes,
		       success_rate, schema_fail_rate, p95_latency_ms, mean_cost_usd,
		       approval_rate, incident_count, operator_rating, rollout_state,
		       availability_state, availability_reason_code, availability_reason,
		       execution_mode, cost_class, health_checked_at,
		       created_by, created_at, updated_at
		FROM capabilities
		WHERE id = $1 AND deleted_at IS NULL
	`, id)
	return scanCapabilityRow(row)
}

// GetForOrg returns a tenant-owned or global capability by ID. A tenant may
// read global catalog entries but may never use this method to read another
// tenant's row.
func (s *CapabilitiesStore) GetForOrg(ctx context.Context, id, orgID string) (*CapabilityRow, error) {
	row := s.pool.QueryRow(ctx, `
		SELECT id, org_id, kind, name, version, description,
		       risk_level, scope, lazy_load, enabled, idempotency_key,
		       schema_input, schema_output, config_json, tags, enabled_for_scopes,
		       success_rate, schema_fail_rate, p95_latency_ms, mean_cost_usd,
		       approval_rate, incident_count, operator_rating, rollout_state,
		       availability_state, availability_reason_code, availability_reason,
		       execution_mode, cost_class, health_checked_at,
		       created_by, created_at, updated_at
		FROM capabilities
		WHERE id = $1 AND (org_id = $2 OR org_id = 'global') AND deleted_at IS NULL
	`, id, orgID)
	return scanCapabilityRow(row)
}

// GetGlobal returns only a process-wide capability row. Global health
// authorities use this exact lookup so a privileged attestation cannot be
// redirected to a tenant-owned row with the same capability identifier.
func (s *CapabilitiesStore) GetGlobal(ctx context.Context, id string) (*CapabilityRow, error) {
	row := s.pool.QueryRow(ctx, `
		SELECT id, org_id, kind, name, version, description,
		       risk_level, scope, lazy_load, enabled, idempotency_key,
		       schema_input, schema_output, config_json, tags, enabled_for_scopes,
		       success_rate, schema_fail_rate, p95_latency_ms, mean_cost_usd,
		       approval_rate, incident_count, operator_rating, rollout_state,
		       availability_state, availability_reason_code, availability_reason,
		       execution_mode, cost_class, health_checked_at,
		       created_by, created_at, updated_at
		FROM capabilities
		WHERE id = $1 AND org_id = 'global' AND deleted_at IS NULL
	`, id)
	return scanCapabilityRow(row)
}

// List returns capabilities, optionally filtered by org, kind, and rollout state.
func (s *CapabilitiesStore) List(ctx context.Context, orgID, kind, rollout string, onlyEnabled bool, limit, offset int) ([]*CapabilityRow, error) {
	if limit <= 0 {
		limit = 50
	}
	args := []any{limit, offset}
	where := "deleted_at IS NULL"
	n := 3

	if orgID != "" {
		where += fmt.Sprintf(" AND (org_id = $%d OR org_id = 'global')", n)
		args = append(args, orgID)
		n++
	}
	if kind != "" {
		where += fmt.Sprintf(" AND kind = $%d", n)
		args = append(args, kind)
		n++
	}
	if rollout != "" {
		where += fmt.Sprintf(" AND rollout_state = $%d", n)
		args = append(args, rollout)
		n++
	}
	if onlyEnabled {
		where += " AND enabled = TRUE"
	}

	// args[0]=limit, args[1]=offset
	rows, err := s.pool.Query(ctx, fmt.Sprintf(`
		SELECT id, org_id, kind, name, version, description,
		       risk_level, scope, lazy_load, enabled, idempotency_key,
		       schema_input, schema_output, config_json, tags, enabled_for_scopes,
		       success_rate, schema_fail_rate, p95_latency_ms, mean_cost_usd,
		       approval_rate, incident_count, operator_rating, rollout_state,
		       availability_state, availability_reason_code, availability_reason,
		       execution_mode, cost_class, health_checked_at,
		       created_by, created_at, updated_at
		FROM capabilities
		WHERE %s
		ORDER BY kind, name
		LIMIT $1 OFFSET $2
	`, where), args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var out []*CapabilityRow
	for rows.Next() {
		r, err := scanCapabilityRowFull(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// SetRolloutState updates the rollout_state and associated timestamp.
func (s *CapabilitiesStore) SetRolloutState(ctx context.Context, id, state, actor string) error {
	now := time.Now().UTC()
	col := ""
	switch state {
	case "quarantine":
		col = ", quarantined_at = $4"
	case "deprecated":
		col = ", deprecated_at = $4"
	case "stable":
		col = ", pinned_at = $4"
	default:
		col = ""
	}
	q := fmt.Sprintf(
		"UPDATE capabilities SET rollout_state=$1, updated_at=$2%s WHERE id=$3 AND deleted_at IS NULL",
		col,
	)
	if col != "" {
		_, err := s.pool.Exec(ctx, q, state, now, id, now)
		return err
	}
	_, err := s.pool.Exec(ctx, q, state, now, id)
	return err
}

// SetRolloutStateForOrg mutates only a capability owned by the verified tenant.
func (s *CapabilitiesStore) SetRolloutStateForOrg(ctx context.Context, id, orgID, state, actor string) error {
	now := time.Now().UTC()
	column := ""
	switch state {
	case "quarantine":
		column = ", quarantined_at = $5"
	case "deprecated":
		column = ", deprecated_at = $5"
	case "stable":
		column = ", pinned_at = $5"
	}
	query := fmt.Sprintf(
		"UPDATE capabilities SET rollout_state=$1, updated_at=$2%s WHERE id=$3 AND org_id=$4 AND deleted_at IS NULL",
		column,
	)
	if column != "" {
		_, err := s.pool.Exec(ctx, query, state, now, id, orgID, now)
		return err
	}
	_, err := s.pool.Exec(ctx, query, state, now, id, orgID)
	return err
}

// AttestAvailabilityForOrg atomically records runtime health and its audit
// event for a tenant-owned capability. Global rows require a separately
// governed operator path and cannot be rewritten by tenant health reporters.
func (s *CapabilitiesStore) AttestAvailabilityForOrg(ctx context.Context, id, orgID, actor string, update AvailabilityUpdate) (bool, error) {
	if id == "" || orgID == "" || actor == "" || update.ExpectedVersion == "" {
		return false, fmt.Errorf("capability, version, organization, and actor are required")
	}
	diffJSON, err := json.Marshal(map[string]any{
		"state":             update.State,
		"version":           update.ExpectedVersion,
		"reason_code":       update.ReasonCode,
		"execution_mode":    update.ExecutionMode,
		"cost_class":        update.CostClass,
		"health_checked_at": update.HealthCheckedAt.UTC(),
	})
	if err != nil {
		return false, fmt.Errorf("marshal availability audit: %w", err)
	}
	auditID := "ral_" + uuid.NewString()
	result, err := s.pool.Exec(ctx, `
		WITH updated AS (
		UPDATE capabilities
		SET availability_state = $1,
		    availability_reason_code = $2,
		    availability_reason = $3,
		    execution_mode = $4,
		    cost_class = $5,
		    health_checked_at = $6,
		    updated_at = $6
		WHERE id = $7 AND org_id = $8 AND version = $9 AND deleted_at IS NULL
		  AND (health_checked_at IS NULL OR health_checked_at < $6)
		RETURNING id, org_id
		)
		INSERT INTO registry_audit_log
		    (id, entity_kind, entity_id, action, actor, org_id, diff_json)
		SELECT $10, 'capability', updated.id, 'availability_attested', $11, updated.org_id, $12::jsonb
		FROM updated
	`, update.State, update.ReasonCode, update.Reason, update.ExecutionMode,
		update.CostClass, update.HealthCheckedAt.UTC(), id, orgID, update.ExpectedVersion, auditID, actor, diffJSON)
	if err != nil {
		return false, err
	}
	return result.RowsAffected() == 1, nil
}

// AttestAvailabilityGlobal atomically records a health attestation for a
// process-wide capability. It is deliberately separate from the tenant path:
// only the dedicated global health-authority scope reaches this method, and
// the query cannot be redirected to a tenant-owned capability row.
func (s *CapabilitiesStore) AttestAvailabilityGlobal(ctx context.Context, id, actor string, update AvailabilityUpdate) (bool, error) {
	if id == "" || actor == "" || update.ExpectedVersion == "" {
		return false, fmt.Errorf("capability, version, and actor are required")
	}
	diffJSON, err := json.Marshal(map[string]any{
		"state":             update.State,
		"version":           update.ExpectedVersion,
		"reason_code":       update.ReasonCode,
		"execution_mode":    update.ExecutionMode,
		"cost_class":        update.CostClass,
		"health_checked_at": update.HealthCheckedAt.UTC(),
	})
	if err != nil {
		return false, fmt.Errorf("marshal global availability audit: %w", err)
	}
	auditID := "ral_" + uuid.NewString()
	result, err := s.pool.Exec(ctx, `
		WITH updated AS (
		UPDATE capabilities
		SET availability_state = $1,
		    availability_reason_code = $2,
		    availability_reason = $3,
		    execution_mode = $4,
		    cost_class = $5,
		    health_checked_at = $6,
		    updated_at = $6
		WHERE id = $7 AND org_id = 'global' AND version = $8 AND deleted_at IS NULL
		  AND (health_checked_at IS NULL OR health_checked_at < $6)
		RETURNING id, org_id
		)
		INSERT INTO registry_audit_log
		    (id, entity_kind, entity_id, action, actor, org_id, diff_json)
		SELECT $9, 'capability', updated.id, 'global_availability_attested', $10, updated.org_id, $11::jsonb
		FROM updated
	`, update.State, update.ReasonCode, update.Reason, update.ExecutionMode,
		update.CostClass, update.HealthCheckedAt.UTC(), id, update.ExpectedVersion, auditID, actor, diffJSON)
	if err != nil {
		return false, err
	}
	return result.RowsAffected() == 1, nil
}

// SoftDelete marks a capability as deleted without removing it.
func (s *CapabilitiesStore) SoftDelete(ctx context.Context, id string) error {
	now := time.Now().UTC()
	_, err := s.pool.Exec(ctx,
		"UPDATE capabilities SET deleted_at=$1, updated_at=$1 WHERE id=$2 AND deleted_at IS NULL",
		now, id,
	)
	return err
}

// SoftDeleteForOrg marks only a verified tenant's capability as deleted.
func (s *CapabilitiesStore) SoftDeleteForOrg(ctx context.Context, id, orgID string) error {
	now := time.Now().UTC()
	_, err := s.pool.Exec(ctx,
		"UPDATE capabilities SET deleted_at=$1, updated_at=$1 WHERE id=$2 AND org_id=$3 AND deleted_at IS NULL",
		now, id, orgID,
	)
	return err
}

// AppendAuditLog writes a single registry_audit_log entry.
func (s *CapabilitiesStore) AppendAuditLog(ctx context.Context, entityKind, entityID, action, actor, orgID string, diffJSON []byte) error {
	id := fmt.Sprintf("ral_%d", time.Now().UnixNano())
	if diffJSON == nil {
		diffJSON = []byte("{}")
	}
	_, err := s.pool.Exec(ctx, `
		INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
		VALUES ($1,$2,$3,$4,$5,$6,$7)
	`, id, entityKind, entityID, action, actor, orgID, diffJSON)
	return err
}

// AuditLogEntry is one registry_audit_log row, newest-first when listed.
type AuditLogEntry struct {
	ID         string          `json:"id"`
	EntityKind string          `json:"entity_kind"`
	EntityID   string          `json:"entity_id"`
	Action     string          `json:"action"`
	Actor      string          `json:"actor"`
	OrgID      string          `json:"org_id"`
	Diff       json.RawMessage `json:"diff"`
	Ts         time.Time       `json:"ts"`
}

// QueryAuditLog returns audit entries newest-first, optionally filtered by
// entity_kind and entity_id (empty string = no filter on that field). `limit`
// is clamped to [1, 500] (default 100). Parameterized — no SQL injection.
func (s *CapabilitiesStore) QueryAuditLog(ctx context.Context, entityKind, entityID string, limit int) ([]AuditLogEntry, error) {
	return s.QueryAuditLogForOrg(ctx, "", entityKind, entityID, limit)
}

// QueryAuditLogForOrg returns audit entries only for the verified tenant. An
// empty orgID preserves the trusted internal behavior used by legacy tests.
func (s *CapabilitiesStore) QueryAuditLogForOrg(ctx context.Context, orgID, entityKind, entityID string, limit int) ([]AuditLogEntry, error) {
	if limit <= 0 {
		limit = 100
	}
	if limit > 500 {
		limit = 500
	}
	rows, err := s.pool.Query(ctx, `
		SELECT id, entity_kind, entity_id, action, actor, org_id, diff_json, ts
		FROM registry_audit_log
		WHERE ($1 = '' OR org_id = $1)
		  AND ($2 = '' OR entity_kind = $2)
		  AND ($3 = '' OR entity_id = $3)
		ORDER BY ts DESC
		LIMIT $4
	`, orgID, entityKind, entityID, limit)
	if err != nil {
		return nil, fmt.Errorf("query audit log: %w", err)
	}
	defer rows.Close()

	entries := make([]AuditLogEntry, 0, limit)
	for rows.Next() {
		var e AuditLogEntry
		var diff []byte
		if err := rows.Scan(&e.ID, &e.EntityKind, &e.EntityID, &e.Action, &e.Actor, &e.OrgID, &diff, &e.Ts); err != nil {
			return nil, fmt.Errorf("scan audit log row: %w", err)
		}
		e.Diff = json.RawMessage(diff)
		entries = append(entries, e)
	}
	return entries, rows.Err()
}

// ScoredCapability pairs a capability row with its composite rank in [0,1].
type ScoredCapability struct {
	Row   *CapabilityRow `json:"capability"`
	Score float64        `json:"score"`
}

// Score computes the composite rank for a single row from its health columns
// and rollout state under the default scoring policy.
func (r *CapabilityRow) Score() float64 {
	return scoring.Score(scoring.Metrics{
		SuccessRate:    r.SuccessRate,
		SchemaFailRate: r.SchemaFailRate,
		P95LatencyMS:   r.P95LatencyMS,
		MeanCostUSD:    r.MeanCostUSD,
		ApprovalRate:   r.ApprovalRate,
		IncidentCount:  r.IncidentCount,
		OperatorRating: r.OperatorRating,
		RolloutState:   r.RolloutState,
	}, scoring.DefaultWeights())
}

// RankedList returns non-deleted capabilities for the org (plus
// 'global'), optionally filtered by kind, ordered by descending composite score
// with enabled entries first (ties broken by kind, then name for stable output).
// Disabled entries remain discoverable with an explicit machine-readable state
// instead of silently disappearing. When ids is non-nil, only
// capabilities whose id is in the set are returned — this is how scope
// resolution (ScopeStore.ResolveForScopeForOrg) narrows the ranked catalog to
// what a verified tenant's agent/org is actually granted. limit <= 0 defaults
// to 50.
func (s *CapabilitiesStore) RankedList(ctx context.Context, orgID, kind string, ids []string, limit int) ([]ScoredCapability, error) {
	if limit <= 0 {
		limit = 50
	}

	where := "deleted_at IS NULL"
	args := []any{}
	n := 1
	if orgID != "" {
		where += fmt.Sprintf(" AND (org_id = $%d OR org_id = 'global')", n)
		args = append(args, orgID)
		n++
	}
	if kind != "" {
		where += fmt.Sprintf(" AND kind = $%d", n)
		args = append(args, kind)
		n++
	}
	if ids != nil {
		where += fmt.Sprintf(" AND id = ANY($%d)", n)
		args = append(args, ids)
		n++
	}

	rows, err := s.pool.Query(ctx, fmt.Sprintf(`
		SELECT id, org_id, kind, name, version, description,
		       risk_level, scope, lazy_load, enabled, idempotency_key,
		       schema_input, schema_output, config_json, tags, enabled_for_scopes,
		       success_rate, schema_fail_rate, p95_latency_ms, mean_cost_usd,
		       approval_rate, incident_count, operator_rating, rollout_state,
		       availability_state, availability_reason_code, availability_reason,
		       execution_mode, cost_class, health_checked_at,
		       created_by, created_at, updated_at
		FROM capabilities
		WHERE %s
	`, where), args...)
	if err != nil {
		return nil, fmt.Errorf("ranked list query: %w", err)
	}
	defer rows.Close()

	scored := make([]ScoredCapability, 0)
	for rows.Next() {
		r, err := scanCapabilityRowFull(rows)
		if err != nil {
			return nil, err
		}
		scored = append(scored, ScoredCapability{Row: r, Score: r.Score()})
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}

	sort.SliceStable(scored, func(i, j int) bool {
		if scored[i].Row.Enabled != scored[j].Row.Enabled {
			return scored[i].Row.Enabled
		}
		if scored[i].Score != scored[j].Score {
			return scored[i].Score > scored[j].Score
		}
		if scored[i].Row.Kind != scored[j].Row.Kind {
			return scored[i].Row.Kind < scored[j].Row.Kind
		}
		return scored[i].Row.Name < scored[j].Row.Name
	})

	if len(scored) > limit {
		scored = scored[:limit]
	}
	return scored, nil
}

// -- helpers ------------------------------------------------------------------

type pgxScanner interface {
	Scan(dest ...any) error
}

func scanCapabilityRow(row pgxScanner) (*CapabilityRow, error) {
	r := &CapabilityRow{}
	err := row.Scan(
		&r.ID, &r.OrgID, &r.Kind, &r.Name, &r.Version, &r.Description,
		&r.RiskLevel, &r.Scope, &r.LazyLoad, &r.Enabled, &r.IdempotencyKey,
		&r.SchemaInput, &r.SchemaOutput, &r.ConfigJSON, &r.Tags, &r.EnabledForScopes,
		&r.SuccessRate, &r.SchemaFailRate, &r.P95LatencyMS, &r.MeanCostUSD,
		&r.ApprovalRate, &r.IncidentCount, &r.OperatorRating, &r.RolloutState,
		&r.AvailabilityState, &r.ReasonCode, &r.Reason,
		&r.ExecutionMode, &r.CostClass, &r.HealthCheckedAt,
		&r.CreatedBy, &r.CreatedAt, &r.UpdatedAt,
	)
	if err != nil {
		return nil, err
	}
	return r, nil
}

func scanCapabilityRowFull(row pgxScanner) (*CapabilityRow, error) {
	return scanCapabilityRow(row)
}

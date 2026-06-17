// Package registry — CapabilitiesStore: full CRUD for the capabilities table.
//
// Separate from CapabilitiesSource (read-only, implements Source) so callers
// that only need to query the registry don't take a write-capable handle.
package registry

import (
	"context"
	"encoding/json"
	"fmt"
	"sort"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/capability-core/internal/scoring"
)

// CapabilityRow is the full mutable row for the capabilities table.
type CapabilityRow struct {
	ID               string
	OrgID            string
	Kind             string
	Name             string
	Version          string
	Description      string
	RiskLevel        string
	Scope            string
	LazyLoad         bool
	Enabled          bool
	IdempotencyKey   string
	SchemaInput      []byte
	SchemaOutput     []byte
	ConfigJSON       []byte
	Tags             []string
	EnabledForScopes []string
	SuccessRate      float64
	SchemaFailRate   float64
	P95LatencyMS     float64
	MeanCostUSD      float64
	ApprovalRate     float64
	IncidentCount    int
	OperatorRating   float64
	RolloutState     string
	CreatedBy        string
	CreatedAt        time.Time
	UpdatedAt        time.Time
}

// CapabilitiesStore provides CRUD access to the capabilities table.
type CapabilitiesStore struct {
	pool *pgxpool.Pool
}

// NewCapabilitiesStore constructs a store.
func NewCapabilitiesStore(pool *pgxpool.Pool) (*CapabilitiesStore, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool required")
	}
	return &CapabilitiesStore{pool: pool}, nil
}

// Upsert inserts or updates a capability row using ON CONFLICT on (org_id,kind,name).
func (s *CapabilitiesStore) Upsert(ctx context.Context, r *CapabilityRow) error {
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
			updated_at       = EXCLUDED.updated_at
	`,
		r.ID, r.OrgID, r.Kind, r.Name, r.Version, r.Description,
		r.RiskLevel, r.Scope, r.LazyLoad, r.Enabled, r.IdempotencyKey,
		r.SchemaInput, r.SchemaOutput, r.ConfigJSON, r.Tags, r.EnabledForScopes,
		r.RolloutState, r.CreatedBy, now, now,
	)
	return err
}

// Get returns a single capability by ID.
func (s *CapabilitiesStore) Get(ctx context.Context, id string) (*CapabilityRow, error) {
	row := s.pool.QueryRow(ctx, `
		SELECT id, org_id, kind, name, version, description,
		       risk_level, scope, lazy_load, enabled, idempotency_key,
		       schema_input, schema_output, config_json, tags, enabled_for_scopes,
		       success_rate, schema_fail_rate, p95_latency_ms, mean_cost_usd,
		       approval_rate, incident_count, operator_rating, rollout_state,
		       created_by, created_at, updated_at
		FROM capabilities
		WHERE id = $1 AND deleted_at IS NULL
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

// SoftDelete marks a capability as deleted without removing it.
func (s *CapabilitiesStore) SoftDelete(ctx context.Context, id string) error {
	now := time.Now().UTC()
	_, err := s.pool.Exec(ctx,
		"UPDATE capabilities SET deleted_at=$1, updated_at=$1 WHERE id=$2 AND deleted_at IS NULL",
		now, id,
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
	if limit <= 0 {
		limit = 100
	}
	if limit > 500 {
		limit = 500
	}
	rows, err := s.pool.Query(ctx, `
		SELECT id, entity_kind, entity_id, action, actor, org_id, diff_json, ts
		FROM registry_audit_log
		WHERE ($1 = '' OR entity_kind = $1) AND ($2 = '' OR entity_id = $2)
		ORDER BY ts DESC
		LIMIT $3
	`, entityKind, entityID, limit)
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

// RankedList returns enabled, non-deleted capabilities for the org (plus
// 'global'), optionally filtered by kind, ordered by descending composite score
// (ties broken by kind, then name for stable output). When ids is non-nil, only
// capabilities whose id is in the set are returned — this is how scope
// resolution (ScopeStore.ResolveForScope) narrows the ranked catalog to what an
// (org, agent) is actually granted. limit <= 0 defaults to 50.
func (s *CapabilitiesStore) RankedList(ctx context.Context, orgID, kind string, ids []string, limit int) ([]ScoredCapability, error) {
	if limit <= 0 {
		limit = 50
	}

	where := "deleted_at IS NULL AND enabled = TRUE"
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

// Package registry — Postgres-backed model catalog.
//
// ModelsRegistry stores model capabilities in the `models` table and exposes
// them as *models.Capability values so the gRPC server can merge them with the
// static seed catalog. Models are scoped per org_id (or "global") and soft
// deleted via deleted_at.
package registry

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgtype"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// idempotencyPrefix matches the static seed prefix so model capability
// idempotency keys remain stable across the merged catalog.
const idempotencyPrefix = "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:"

// Model is the persisted row representation.
type Model struct {
	ID          uuid.UUID
	OrgID       uuid.UUID
	Scope       string
	Provider    string
	Name        string
	Version     string
	ConfigJSON  []byte
	Enabled     bool
	RiskLevel   string
	LazyLoad    bool
	Description string
	// PrivacyTier and Residency are declarative disclosure (migration 0013):
	// startup-cached, never consulted synchronously on the invoke hot path
	// (PROVIDER_AND_PRIVACY_STRATEGY.md §4.5). PrivacyTier is one of
	// models.PrivacyTier*; Residency is a free-form declared label ("eu",
	// "norway") or empty when undeclared.
	PrivacyTier string
	Residency   string
}

// ModelsFilter narrows List results.
type ModelsFilter struct {
	OrgID       *uuid.UUID
	Scope       string
	Provider    string
	OnlyEnabled bool
}

// ModelsRegistry is the pgx-backed model catalog.
type ModelsRegistry struct {
	pool *pgxpool.Pool
}

// NewModelsRegistry constructs a ModelsRegistry. pool must not be nil.
func NewModelsRegistry(pool *pgxpool.Pool) (*ModelsRegistry, error) {
	if pool == nil {
		return nil, domain.ErrInvalidArgument
	}
	return &ModelsRegistry{pool: pool}, nil
}

// CapabilityID returns the registry-style capability ID for a model row.
func CapabilityID(provider, name string) string {
	return fmt.Sprintf("cap.model.%s.%s", provider, name)
}

// ToCapability projects a Model row onto the Capability domain type.
func ToCapability(m *Model) *models.Capability {
	if m == nil {
		return nil
	}
	id := CapabilityID(m.Provider, m.Name)
	scope := m.Scope
	if scope == "" {
		scope = "global"
	}
	return &models.Capability{
		ID:               id,
		Name:             m.Name,
		Kind:             models.KindModel,
		Version:          m.Version,
		Description:      m.Description,
		RiskLevel:        m.RiskLevel,
		LazyLoad:         m.LazyLoad,
		Scope:            scope,
		Enabled:          m.Enabled,
		IdempotencyKey:   idempotencyPrefix + id,
		OrgID:            m.OrgID.String(),
		EnabledForScopes: []string{scope},
		PrivacyTier:      m.PrivacyTier,
		Residency:        m.Residency,
	}
}

const modelColumns = `id, org_id, scope, provider, name, version,
	config_json, enabled, risk_level, lazy_load, description, privacy_tier, residency`

func scanModel(row pgx.Row) (*Model, error) {
	var (
		m     Model
		orgID pgtype.UUID
	)
	if err := row.Scan(
		&m.ID, &orgID, &m.Scope, &m.Provider, &m.Name, &m.Version,
		&m.ConfigJSON, &m.Enabled, &m.RiskLevel, &m.LazyLoad, &m.Description,
		&m.PrivacyTier, &m.Residency,
	); err != nil {
		return nil, err
	}
	if orgID.Valid {
		m.OrgID = uuid.UUID(orgID.Bytes)
	} else {
		m.OrgID = uuid.Nil
	}
	return &m, nil
}

// Get returns a single model by id. Returns domain.ErrCapabilityNotFound when absent.
func (r *ModelsRegistry) Get(ctx context.Context, id uuid.UUID) (*Model, error) {
	row := r.pool.QueryRow(ctx, `
		SELECT `+modelColumns+`
		FROM models
		WHERE id = $1 AND deleted_at IS NULL`, id)
	m, err := scanModel(row)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, domain.ErrCapabilityNotFound
		}
		return nil, err
	}
	return m, nil
}

// GetByName returns a single non-deleted model by (org_id, provider, name).
// uuid.Nil is treated as the global scope (org_id IS NULL).
func (r *ModelsRegistry) GetByName(ctx context.Context, orgID uuid.UUID, provider, name string) (*Model, error) {
	var row pgx.Row
	if orgID == uuid.Nil {
		row = r.pool.QueryRow(ctx, `
			SELECT `+modelColumns+`
			FROM models
			WHERE org_id IS NULL AND provider = $1 AND name = $2 AND deleted_at IS NULL`,
			provider, name)
	} else {
		row = r.pool.QueryRow(ctx, `
			SELECT `+modelColumns+`
			FROM models
			WHERE org_id = $1 AND provider = $2 AND name = $3 AND deleted_at IS NULL`,
			orgID, provider, name)
	}
	m, err := scanModel(row)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, domain.ErrCapabilityNotFound
		}
		return nil, err
	}
	return m, nil
}

// GetByCapabilityID parses a capability id of the form "cap.model.<provider>.<name>"
// and returns the matching global (org_id IS NULL) model.
func (r *ModelsRegistry) GetByCapabilityID(ctx context.Context, id string) (*Model, error) {
	return r.GetByCapabilityIDForOrg(ctx, id, "")
}

// GetByCapabilityIDForOrg resolves a tenant-owned model first, then the global
// fallback. An invalid or empty org identifier can only resolve global rows.
func (r *ModelsRegistry) GetByCapabilityIDForOrg(ctx context.Context, id, orgID string) (*Model, error) {
	const prefix = "cap.model."
	if !strings.HasPrefix(id, prefix) {
		return nil, domain.ErrCapabilityNotFound
	}
	rest := id[len(prefix):]
	dot := strings.Index(rest, ".")
	if dot <= 0 || dot == len(rest)-1 {
		return nil, domain.ErrInvalidArgument
	}
	provider, name := rest[:dot], rest[dot+1:]
	if parsedOrgID, err := uuid.Parse(orgID); err == nil && parsedOrgID != uuid.Nil {
		model, getErr := r.GetByName(ctx, parsedOrgID, provider, name)
		if getErr == nil {
			return model, nil
		}
		if !errors.Is(getErr, domain.ErrCapabilityNotFound) {
			return nil, getErr
		}
	}
	return r.GetByName(ctx, uuid.Nil, provider, name)
}

// List returns all non-deleted models matching the filter. Sorted by
// (provider, name) for deterministic output.
func (r *ModelsRegistry) List(ctx context.Context, f ModelsFilter) ([]*Model, error) {
	q := `SELECT ` + modelColumns + ` FROM models WHERE deleted_at IS NULL`
	args := []any{}
	if f.OrgID != nil {
		args = append(args, *f.OrgID)
		q += fmt.Sprintf(" AND org_id = $%d", len(args))
	}
	if f.Scope != "" {
		args = append(args, f.Scope)
		q += fmt.Sprintf(" AND scope = $%d", len(args))
	}
	if f.Provider != "" {
		args = append(args, f.Provider)
		q += fmt.Sprintf(" AND provider = $%d", len(args))
	}
	if f.OnlyEnabled {
		q += " AND enabled = TRUE"
	}
	q += " ORDER BY provider, name"

	rows, err := r.pool.Query(ctx, q, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var out []*Model
	for rows.Next() {
		m, err := scanModel(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, m)
	}
	return out, rows.Err()
}

// ListAsCapabilities is a convenience wrapper that projects List results onto
// *models.Capability. Returns an empty slice (never nil) on success.
func (r *ModelsRegistry) ListAsCapabilities(ctx context.Context, f ModelsFilter) ([]*models.Capability, error) {
	rows, err := r.List(ctx, f)
	if err != nil {
		return nil, err
	}
	out := make([]*models.Capability, 0, len(rows))
	for _, m := range rows {
		out = append(out, ToCapability(m))
	}
	return out, nil
}

// ListAsCapabilitiesForOrg returns only the verified tenant's models plus
// global fallbacks. Invalid/empty tenant identifiers receive global models.
func (r *ModelsRegistry) ListAsCapabilitiesForOrg(ctx context.Context, orgID string) ([]*models.Capability, error) {
	query := `SELECT ` + modelColumns + ` FROM models WHERE deleted_at IS NULL AND enabled = TRUE AND org_id IS NULL ORDER BY provider, name`
	args := []any{}
	if parsedOrgID, err := uuid.Parse(orgID); err == nil && parsedOrgID != uuid.Nil {
		query = `SELECT ` + modelColumns + ` FROM models WHERE deleted_at IS NULL AND enabled = TRUE AND (org_id IS NULL OR org_id = $1) ORDER BY provider, name`
		args = append(args, parsedOrgID)
	}
	rows, err := r.pool.Query(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := make([]*models.Capability, 0)
	for rows.Next() {
		model, scanErr := scanModel(rows)
		if scanErr != nil {
			return nil, scanErr
		}
		out = append(out, ToCapability(model))
	}
	return out, rows.Err()
}

// Upsert inserts a new model or updates an existing one keyed by
// (org_id, provider, name). config_json must be valid JSON; pass `nil` for an
// empty object.
func (r *ModelsRegistry) Upsert(ctx context.Context, m *Model) (*Model, error) {
	if m == nil {
		return nil, domain.ErrInvalidArgument
	}
	if m.Provider == "" || m.Name == "" {
		return nil, domain.ErrInvalidArgument
	}
	if m.ID == uuid.Nil {
		m.ID = uuid.New()
	}
	if m.Scope == "" {
		m.Scope = "global"
	}
	if m.RiskLevel == "" {
		m.RiskLevel = models.RiskLow
	}
	if !models.IsSupportedRiskLevel(m.RiskLevel) {
		return nil, domain.ErrInvalidArgument
	}
	if m.PrivacyTier == "" {
		m.PrivacyTier = models.PrivacyTierUnspecified
	}
	if !models.IsSupportedPrivacyTier(m.PrivacyTier) {
		return nil, domain.ErrInvalidArgument
	}
	cfg := m.ConfigJSON
	if len(cfg) == 0 {
		cfg = []byte("{}")
	}
	var orgArg any
	if m.OrgID != uuid.Nil {
		orgArg = m.OrgID
	}
	row := r.pool.QueryRow(ctx, `
		INSERT INTO models (
			id, org_id, scope, provider, name, version,
			config_json, enabled, risk_level, lazy_load, description,
			privacy_tier, residency
		) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
		ON CONFLICT (org_id, provider, name) WHERE deleted_at IS NULL
		DO UPDATE SET
			scope        = EXCLUDED.scope,
			version      = EXCLUDED.version,
			config_json  = EXCLUDED.config_json,
			enabled      = EXCLUDED.enabled,
			risk_level   = EXCLUDED.risk_level,
			lazy_load    = EXCLUDED.lazy_load,
			description  = EXCLUDED.description,
			privacy_tier = EXCLUDED.privacy_tier,
			residency    = EXCLUDED.residency,
			updated_at   = now()
		RETURNING `+modelColumns,
		m.ID, orgArg, m.Scope, m.Provider, m.Name, m.Version,
		cfg, m.Enabled, m.RiskLevel, m.LazyLoad, m.Description,
		m.PrivacyTier, m.Residency,
	)
	return scanModel(row)
}

// Delete soft-deletes a model by id. Returns domain.ErrCapabilityNotFound when no row
// is affected.
func (r *ModelsRegistry) Delete(ctx context.Context, id uuid.UUID) error {
	tag, err := r.pool.Exec(ctx, `
		UPDATE models SET deleted_at = now(), updated_at = now()
		WHERE id = $1 AND deleted_at IS NULL`, id)
	if err != nil {
		return err
	}
	if tag.RowsAffected() == 0 {
		return domain.ErrCapabilityNotFound
	}
	return nil
}

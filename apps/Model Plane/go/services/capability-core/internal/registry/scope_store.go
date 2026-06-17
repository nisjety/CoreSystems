// Package registry — ScopeStore: durable CRUD + resolution over the
// capability_scopes table (migration 0003).
//
// capability_scopes records explicit grants: "capability C is enabled for
// scope_kind K with scope_value V" (V='*' means "all of K"). This is the
// per-(org,agent) authority layer the policy engine consults — distinct from
// the capability's own static enabled_for_scopes array, which only declares
// *which kinds of scope* a capability supports, not *who* is granted it.
//
// Resolution answers: "given (org_id, agent_id), which capabilities may be
// used?" by matching grants whose (scope_kind, scope_value) cover the caller's
// org and agent, plus any wildcard ('*') grant.
package registry

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// Scope kind constants (mirror the proto/EvaluatePolicy canonical values).
const (
	ScopeKindOrg       = "org"
	ScopeKindAgent     = "agent"
	ScopeKindWorkspace = "workspace"
	ScopeKindUser      = "user"
	ScopeKindGlobal    = "global"

	// scopeValueAll is the wildcard scope_value meaning "all of this kind".
	scopeValueAll = "*"
)

// ScopeGrant is a single capability_scopes row.
type ScopeGrant struct {
	ID           string     `json:"id"`
	CapabilityID string     `json:"capability_id"`
	ScopeKind    string     `json:"scope_kind"`
	ScopeValue   string     `json:"scope_value"`
	GrantedBy    string     `json:"granted_by"`
	GrantedAt    time.Time  `json:"granted_at"`
	RevokedAt    *time.Time `json:"revoked_at,omitempty"`
}

// ScopeStore provides CRUD + resolution over capability_scopes.
type ScopeStore struct {
	pool *pgxpool.Pool
}

// NewScopeStore constructs a ScopeStore.
func NewScopeStore(pool *pgxpool.Pool) (*ScopeStore, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool required")
	}
	return &ScopeStore{pool: pool}, nil
}

// Grant inserts an active scope grant for a capability. scopeValue "" is
// normalised to the wildcard "*". The grant id is caller-supplied; pass "" to
// have one generated. Returns the persisted grant.
func (s *ScopeStore) Grant(ctx context.Context, id, capabilityID, scopeKind, scopeValue, grantedBy string) (*ScopeGrant, error) {
	if capabilityID == "" || scopeKind == "" {
		return nil, fmt.Errorf("capability_id and scope_kind are required")
	}
	if scopeValue == "" {
		scopeValue = scopeValueAll
	}
	if id == "" {
		id = fmt.Sprintf("scope_%d", time.Now().UnixNano())
	}
	now := time.Now().UTC()
	_, err := s.pool.Exec(ctx, `
		INSERT INTO capability_scopes (id, capability_id, scope_kind, scope_value, granted_by, granted_at)
		VALUES ($1,$2,$3,$4,$5,$6)
	`, id, capabilityID, scopeKind, scopeValue, grantedBy, now)
	if err != nil {
		return nil, fmt.Errorf("grant scope: %w", err)
	}
	return &ScopeGrant{
		ID: id, CapabilityID: capabilityID, ScopeKind: scopeKind,
		ScopeValue: scopeValue, GrantedBy: grantedBy, GrantedAt: now,
	}, nil
}

// Revoke marks all active grants for (capabilityID, scopeKind, scopeValue) as
// revoked. scopeValue "" is normalised to the wildcard. Returns the number of
// grants revoked.
func (s *ScopeStore) Revoke(ctx context.Context, capabilityID, scopeKind, scopeValue string) (int64, error) {
	if capabilityID == "" || scopeKind == "" {
		return 0, fmt.Errorf("capability_id and scope_kind are required")
	}
	if scopeValue == "" {
		scopeValue = scopeValueAll
	}
	tag, err := s.pool.Exec(ctx, `
		UPDATE capability_scopes
		SET revoked_at = now()
		WHERE capability_id = $1 AND scope_kind = $2 AND scope_value = $3 AND revoked_at IS NULL
	`, capabilityID, scopeKind, scopeValue)
	if err != nil {
		return 0, fmt.Errorf("revoke scope: %w", err)
	}
	return tag.RowsAffected(), nil
}

// ListForCapability returns active grants for a single capability.
func (s *ScopeStore) ListForCapability(ctx context.Context, capabilityID string) ([]ScopeGrant, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT id, capability_id, scope_kind, scope_value, granted_by, granted_at, revoked_at
		FROM capability_scopes
		WHERE capability_id = $1 AND revoked_at IS NULL
		ORDER BY granted_at DESC
	`, capabilityID)
	if err != nil {
		return nil, fmt.Errorf("list scopes: %w", err)
	}
	defer rows.Close()
	return scanScopeGrants(rows)
}

// IsGrantedForScope reports whether a capability has an active grant covering
// the (scopeKind, scopeValue) request. A grant with scope_value '*' covers any
// value of that kind; an exact scope_value match also covers it. Empty inputs
// yield false (caller must supply both).
func (s *ScopeStore) IsGrantedForScope(ctx context.Context, capabilityID, scopeKind, scopeValue string) (bool, error) {
	if capabilityID == "" || scopeKind == "" || scopeValue == "" {
		return false, nil
	}
	var exists bool
	err := s.pool.QueryRow(ctx, `
		SELECT EXISTS (
			SELECT 1 FROM capability_scopes
			WHERE capability_id = $1
			  AND scope_kind = $2
			  AND (scope_value = $3 OR scope_value = '*')
			  AND revoked_at IS NULL
		)
	`, capabilityID, scopeKind, scopeValue).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("is granted for scope: %w", err)
	}
	return exists, nil
}

// HasAnyGrants reports whether a capability has at least one active scope grant
// of the given kind. Used by the policy engine to decide whether the durable
// grant table governs this capability at all (no grants of a kind => the table
// is not opted-in for that kind, fall back to static enabled_for_scopes).
func (s *ScopeStore) HasAnyGrants(ctx context.Context, capabilityID, scopeKind string) (bool, error) {
	if capabilityID == "" || scopeKind == "" {
		return false, nil
	}
	var exists bool
	err := s.pool.QueryRow(ctx, `
		SELECT EXISTS (
			SELECT 1 FROM capability_scopes
			WHERE capability_id = $1 AND scope_kind = $2 AND revoked_at IS NULL
		)
	`, capabilityID, scopeKind).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("has any grants: %w", err)
	}
	return exists, nil
}

// ResolveForScope returns the set of capability IDs that have an active grant
// covering (scopeKind, scopeValue) — exact value or wildcard. This is the
// "which capabilities may (org/agent X) use?" query backing the agentic loop's
// scoped catalog. Pass scopeValue '*' to enumerate all capabilities granted to
// the kind at large.
func (s *ScopeStore) ResolveForScope(ctx context.Context, scopeKind, scopeValue string) ([]string, error) {
	if scopeKind == "" {
		return nil, fmt.Errorf("scope_kind is required")
	}
	if scopeValue == "" {
		scopeValue = scopeValueAll
	}
	rows, err := s.pool.Query(ctx, `
		SELECT DISTINCT capability_id
		FROM capability_scopes
		WHERE scope_kind = $1
		  AND (scope_value = $2 OR scope_value = '*')
		  AND revoked_at IS NULL
		ORDER BY capability_id
	`, scopeKind, scopeValue)
	if err != nil {
		return nil, fmt.Errorf("resolve for scope: %w", err)
	}
	defer rows.Close()

	var ids []string
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return nil, fmt.Errorf("scan resolved capability id: %w", err)
		}
		ids = append(ids, id)
	}
	return ids, rows.Err()
}

func scanScopeGrants(rows interface {
	Next() bool
	Scan(...any) error
	Err() error
}) ([]ScopeGrant, error) {
	var out []ScopeGrant
	for rows.Next() {
		var g ScopeGrant
		if err := rows.Scan(&g.ID, &g.CapabilityID, &g.ScopeKind, &g.ScopeValue, &g.GrantedBy, &g.GrantedAt, &g.RevokedAt); err != nil {
			return nil, fmt.Errorf("scan scope grant: %w", err)
		}
		out = append(out, g)
	}
	return out, rows.Err()
}

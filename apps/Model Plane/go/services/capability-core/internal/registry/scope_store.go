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

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

// Scope kind constants (mirror the proto/EvaluatePolicy canonical values).
const (
	ScopeKindRun       = "run"
	ScopeKindThread    = "thread"
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
	OrgID        string     `json:"org_id"`
	CapabilityID string     `json:"capability_id"`
	ScopeKind    string     `json:"scope_kind"`
	ScopeValue   string     `json:"scope_value"`
	GrantedBy    string     `json:"granted_by"`
	GrantedAt    time.Time  `json:"granted_at"`
	RevokedAt    *time.Time `json:"revoked_at,omitempty"`
}

// ScopeStore provides CRUD + resolution over capability_scopes.
type ScopeStore struct {
	pool scopeDatabase
}

type scopeDatabase interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
}

// NewScopeStore constructs a ScopeStore.
func NewScopeStore(pool *pgxpool.Pool) (*ScopeStore, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool required")
	}
	return &ScopeStore{pool: pool}, nil
}

// Grant inserts a tenant-bound active scope grant for a capability. The
// verified org is mandatory even when agent IDs collide between tenants.
func (s *ScopeStore) Grant(ctx context.Context, id, orgID, capabilityID, scopeKind, scopeValue, grantedBy string) (*ScopeGrant, error) {
	if err := validateScopeTuple(orgID, capabilityID, scopeKind, scopeValue); err != nil {
		return nil, err
	}
	if id == "" {
		id = fmt.Sprintf("scope_%d", time.Now().UnixNano())
	}
	now := time.Now().UTC()
	tag, err := s.pool.Exec(ctx, `
		INSERT INTO capability_scopes (id, org_id, capability_id, scope_kind, scope_value, granted_by, granted_at)
		SELECT $1,$2,$3,$4,$5,$6,$7
		FROM capabilities c
		WHERE c.id = $3
		  AND c.deleted_at IS NULL
		  AND (c.org_id = $2 OR c.org_id = 'global')
	`, id, orgID, capabilityID, scopeKind, scopeValue, grantedBy, now)
	if err != nil {
		return nil, fmt.Errorf("grant scope: %w", err)
	}
	if tag.RowsAffected() != 1 {
		return nil, fmt.Errorf("grant scope: capability not found for tenant")
	}
	return &ScopeGrant{
		ID: id, OrgID: orgID, CapabilityID: capabilityID, ScopeKind: scopeKind,
		ScopeValue: scopeValue, GrantedBy: grantedBy, GrantedAt: now,
	}, nil
}

// Revoke marks the verified tenant's active grants for the exact capability,
// scope kind, and scope value as revoked.
func (s *ScopeStore) Revoke(ctx context.Context, orgID, capabilityID, scopeKind, scopeValue string) (int64, error) {
	if err := validateScopeTuple(orgID, capabilityID, scopeKind, scopeValue); err != nil {
		return 0, err
	}
	tag, err := s.pool.Exec(ctx, `
		UPDATE capability_scopes
		SET revoked_at = now()
		WHERE org_id = $1 AND capability_id = $2 AND scope_kind = $3 AND scope_value = $4 AND revoked_at IS NULL
	`, orgID, capabilityID, scopeKind, scopeValue)
	if err != nil {
		return 0, fmt.Errorf("revoke scope: %w", err)
	}
	return tag.RowsAffected(), nil
}

// ListForCapabilityForOrg returns grants for a tenant-owned capability. For a
// global capability it exposes only the verified tenant's explicitly bound
// grants.
func (s *ScopeStore) ListForCapabilityForOrg(ctx context.Context, capabilityID, orgID string) ([]ScopeGrant, error) {
	if capabilityID == "" || orgID == "" {
		return nil, fmt.Errorf("capability_id and org_id are required")
	}
	rows, err := s.pool.Query(ctx, `
		SELECT cs.id, cs.org_id, cs.capability_id, cs.scope_kind, cs.scope_value,
		       cs.granted_by, cs.granted_at, cs.revoked_at
		FROM capability_scopes cs
		JOIN capabilities c ON c.id = cs.capability_id AND c.deleted_at IS NULL
		WHERE cs.capability_id = $1
		  AND cs.org_id = $2
		  AND cs.revoked_at IS NULL
		  AND (c.org_id = $2 OR c.org_id = 'global')
		ORDER BY cs.granted_at DESC
	`, capabilityID, orgID)
	if err != nil {
		return nil, fmt.Errorf("list tenant scopes: %w", err)
	}
	defer rows.Close()
	return scanScopeGrants(rows)
}

// IsGrantedForScope reports whether a capability has an active grant covering
// the (scopeKind, scopeValue) request. A grant with scope_value '*' covers any
// value of that kind; an exact scope_value match also covers it. Empty inputs
// yield false (caller must supply both).
func (s *ScopeStore) IsGrantedForScope(ctx context.Context, capabilityID, orgID, scopeKind, scopeValue string) (bool, error) {
	if capabilityID == "" || orgID == "" || !IsSupportedScopeKind(scopeKind) || scopeValue == "" {
		return false, nil
	}
	var exists bool
	err := s.pool.QueryRow(ctx, `
		SELECT EXISTS (
			SELECT 1 FROM capability_scopes
			WHERE capability_id = $1
			  AND org_id = $2
			  AND scope_kind = $3
			  AND (scope_value = $4 OR scope_value = '*')
			  AND revoked_at IS NULL
		)
	`, capabilityID, orgID, scopeKind, scopeValue).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("is granted for scope: %w", err)
	}
	return exists, nil
}

// HasAnyGrants reports whether a capability has at least one active grant of
// the given kind for the verified tenant. Catalog consumers may use it for
// observability; invocation policy requires an exact grant.
func (s *ScopeStore) HasAnyGrants(ctx context.Context, capabilityID, orgID, scopeKind string) (bool, error) {
	if capabilityID == "" || orgID == "" || !IsSupportedScopeKind(scopeKind) {
		return false, nil
	}
	var exists bool
	err := s.pool.QueryRow(ctx, `
		SELECT EXISTS (
			SELECT 1 FROM capability_scopes
			WHERE capability_id = $1 AND org_id = $2 AND scope_kind = $3 AND revoked_at IS NULL
		)
	`, capabilityID, orgID, scopeKind).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("has any grants: %w", err)
	}
	return exists, nil
}

// ResolveForScopeForOrg restricts resolved capabilities to the verified
// tenant plus global catalog entries. The tenant is mandatory.
func (s *ScopeStore) ResolveForScopeForOrg(ctx context.Context, orgID, scopeKind, scopeValue string) ([]string, error) {
	if orgID == "" || !IsSupportedScopeKind(scopeKind) {
		return nil, fmt.Errorf("valid org_id and scope_kind are required")
	}
	if scopeValue == "" {
		scopeValue = scopeValueAll
	}
	rows, err := s.pool.Query(ctx, `
		SELECT DISTINCT cs.capability_id
		FROM capability_scopes cs
		JOIN capabilities c ON c.id = cs.capability_id AND c.deleted_at IS NULL
		WHERE cs.scope_kind = $1
		  AND (cs.scope_value = $2 OR cs.scope_value = '*')
		  AND cs.revoked_at IS NULL
		  AND cs.org_id = $3
		  AND (c.org_id = $3 OR c.org_id = 'global')
		ORDER BY cs.capability_id
	`, scopeKind, scopeValue, orgID)
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
		if err := rows.Scan(&g.ID, &g.OrgID, &g.CapabilityID, &g.ScopeKind, &g.ScopeValue, &g.GrantedBy, &g.GrantedAt, &g.RevokedAt); err != nil {
			return nil, fmt.Errorf("scan scope grant: %w", err)
		}
		out = append(out, g)
	}
	return out, rows.Err()
}

// IsSupportedScopeKind is the closed set understood by policy evaluation and
// persisted grants.
func IsSupportedScopeKind(scopeKind string) bool {
	switch scopeKind {
	case ScopeKindRun, ScopeKindThread, ScopeKindOrg, ScopeKindAgent,
		ScopeKindWorkspace, ScopeKindUser, ScopeKindGlobal:
		return true
	default:
		return false
	}
}

func validateScopeTuple(orgID, capabilityID, scopeKind, scopeValue string) error {
	if orgID == "" || capabilityID == "" || !IsSupportedScopeKind(scopeKind) || scopeValue == "" {
		return fmt.Errorf("valid org_id, capability_id, scope_kind, and scope_value are required")
	}
	if scopeKind == ScopeKindOrg && scopeValue != orgID {
		return fmt.Errorf("org scope value must match verified org_id")
	}
	return nil
}

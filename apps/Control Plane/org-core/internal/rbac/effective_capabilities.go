package rbac

import (
	"context"
	"errors"
	"strings"
)

// defaultCapabilitiesByBaseRole is a hardcoded, in-code default — NOT a
// migration-seeded org_role_mappings row. There is no live per-request
// capability enforcement anywhere in this codebase yet (the catalog above
// is an admin/role-editor configuration surface only); this is the first.
// Keeping the default in code rather than seeding rows into a live table
// keeps it simple and auditable, and an org can still override it later by
// creating a real custom role whose name happens to match a member's
// organization_members.role value.
var defaultCapabilitiesByBaseRole = map[string][]string{
	"owner": allCatalogKeys(),
	"admin": allCatalogKeys(),
	"member": {
		"org:read",
		"members:read",
		"billing:read",
		"resources:create", "resources:read", "resources:update",
		"integrations:read",
		"support:recurrence:read",
	},
	"viewer": {
		"org:read",
		"members:read",
		"resources:read",
	},
}

func allCatalogKeys() []string {
	keys := make([]string, 0, len(catalog))
	for _, entry := range catalog {
		keys = append(keys, entry.Key)
	}
	return keys
}

// EffectiveCapabilities resolves the capability set for a member identified
// by roleName — the exact value already stored in organization_members.role,
// which the caller (org-core's own organization handlers, via the gateway's
// already-resolved authorized_membership.role) already has in hand. This
// deliberately does not look up the member itself: organization_members.role
// holds either one of the four base tiers (owner/admin/member/viewer) or a
// custom role name, so resolving "what does this role grant" only needs the
// role string, not a fresh membership lookup.
//
// If roleName matches a custom org_role_mappings row for this org, that
// role's Permissions are authoritative (a custom assignment replaces the
// base-tier default, it does not add to it). Otherwise roleName is treated
// as a base tier and falls back to defaultCapabilitiesByBaseRole. An
// unrecognized roleName (should not happen given upstream validation, but
// defensively) yields no capabilities at all — fail closed, not open.
func (r *Repository) EffectiveCapabilities(ctx context.Context, orgID, roleName string) ([]string, error) {
	orgID = strings.TrimSpace(orgID)
	roleName = strings.TrimSpace(roleName)
	if orgID == "" || roleName == "" {
		return nil, nil
	}

	customRole, err := r.Get(ctx, orgID, roleName)
	if err != nil && !errors.Is(err, ErrNotFound) {
		return nil, err
	}
	if customRole != nil {
		return append([]string(nil), customRole.Permissions...), nil
	}

	if defaults, ok := defaultCapabilitiesByBaseRole[strings.ToLower(roleName)]; ok {
		return append([]string(nil), defaults...), nil
	}
	return []string{}, nil
}

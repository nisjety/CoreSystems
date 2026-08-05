// Package rbac owns the role-based access control surface for org-core.
//
// U6-3 (ui-ux-verevon-gap.md §10): exposes CRUD on `org_role_mappings`
// (per-org named roles + JSONB permission list + is_custom flag) plus
// a capability catalog endpoint that returns the valid permission strings
// the UI can present in its role editor.
//
// Wire surface:
//   GET    /orgs/:id/roles                     — list all roles (default + custom)
//   POST   /orgs/:id/roles                     — create a custom role
//   PATCH  /orgs/:id/roles/:roleName           — edit permissions
//   DELETE /orgs/:id/roles/:roleName           — delete custom role (default: 409)
//   GET    /orgs/:id/roles/catalog             — capability catalog
//   PATCH  /orgs/:id/members/:userId/role      — assign role to member
package rbac

import "time"

// Role is one row in `org_role_mappings`. Permissions is the JSONB array
// of capability strings (see CatalogEntry below for the catalog they
// must come from).
type Role struct {
	ID          string    `json:"id"`
	OrgID       string    `json:"org_id"`
	RoleName    string    `json:"role_name"`
	Permissions []string  `json:"permissions"`
	IsCustom    bool      `json:"is_custom"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

// CreateParams is the input to Service.CreateRole. The role name is
// scoped per-org; `permissions` must be a subset of the catalog (the
// repository enforces this with a soft check — empty arrays are allowed).
type CreateParams struct {
	OrgID       string
	RoleName    string
	Permissions []string
}

// UpdateParams is the input to Service.UpdateRole.
type UpdateParams struct {
	OrgID       string
	RoleName    string
	Permissions []string
}

// CatalogEntry is one capability in the org-wide catalog. The catalog is
// hardcoded today (see catalog.go) because the four default roles seeded
// at migration time imply a stable canonical set. New capability strings
// must be added to BOTH the catalog AND any default-role seeds that need
// them — we don't want orphans.
type CatalogEntry struct {
	Key         string `json:"key"`         // e.g. "members:invite"
	Group       string `json:"group"`       // human-readable section, e.g. "Members"
	Label       string `json:"label"`       // human-readable label, e.g. "Invite members"
	Description string `json:"description"` // tooltip / longer copy
}

// MemberRoleAssignment is the response from PATCH /members/:userId/role.
type MemberRoleAssignment struct {
	OrgID  string `json:"org_id"`
	UserID string `json:"user_id"`
	Role   string `json:"role"`
}

package rbac

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/jackc/pgx/v5"
)

// Sentinel errors. Callers map these to HTTP status codes (404, 409, etc.)
var (
	ErrNotFound        = errors.New("role not found")
	ErrAlreadyExists   = errors.New("role already exists")
	ErrCannotDelete    = errors.New("role cannot be deleted (default role)")
	ErrInvalidCapability = errors.New("permissions contains a capability not in the catalog")
)

type Repository struct {
	// db carries WithOrgScope. Every method below is single-org CRUD on
	// org_role_mappings / organization_members, so — same as
	// internal/org/repository.go's single-tenant paths — every query runs
	// inside an org-scoped transaction (SET LOCAL app.current_org + SET
	// LOCAL ROLE org_core_app) so RLS (migration 009/013) enforces a
	// hard DB-level tenant filter, not just the org_id = $1 WHERE clause.
	db *database.DB
}

func NewRepository(db *database.DB) *Repository {
	return &Repository{db: db}
}

const roleColumns = `id, org_id, role_name, permissions, is_custom, created_at, updated_at`

// List returns every role for the org. Always returns the 4 default roles
// first (sorted alphabetically by role_name) then custom roles.
func (r *Repository) List(ctx context.Context, orgID string) ([]Role, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("rbac repository not configured")
	}
	var out []Role
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, `
SELECT `+roleColumns+`
FROM org_role_mappings
WHERE org_id = $1
ORDER BY is_custom, role_name`, orgID)
		if err != nil {
			return fmt.Errorf("list roles: %w", err)
		}
		defer rows.Close()

		out = make([]Role, 0)
		for rows.Next() {
			role, err := scanRole(rows)
			if err != nil {
				return err
			}
			out = append(out, *role)
		}
		return rows.Err()
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// Get returns one role by name.
func (r *Repository) Get(ctx context.Context, orgID, roleName string) (*Role, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("rbac repository not configured")
	}
	var role *Role
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		row := tx.QueryRow(ctx, `
SELECT `+roleColumns+`
FROM org_role_mappings
WHERE org_id = $1 AND role_name = $2`, orgID, roleName)

		scanned, err := scanRole(row)
		if err != nil {
			return err
		}
		role = scanned
		return nil
	})
	if err != nil {
		return nil, err
	}
	return role, nil
}

// Create inserts a new custom role. Returns ErrAlreadyExists on conflict.
// Permission membership is checked against the catalog before insert.
func (r *Repository) Create(ctx context.Context, params CreateParams) (*Role, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("rbac repository not configured")
	}
	orgID := strings.TrimSpace(params.OrgID)
	roleName := strings.TrimSpace(params.RoleName)
	if orgID == "" || roleName == "" {
		return nil, fmt.Errorf("org_id and role_name required")
	}
	if err := validateCapabilities(params.Permissions); err != nil {
		return nil, err
	}

	id := orgID + "_" + roleName + "_" + randomSuffix(6)
	permsJSON, err := json.Marshal(params.Permissions)
	if err != nil {
		return nil, fmt.Errorf("encode permissions: %w", err)
	}

	var role *Role
	err = r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		row := tx.QueryRow(ctx, `
INSERT INTO org_role_mappings (id, org_id, role_name, permissions, is_custom, created_at, updated_at)
VALUES ($1, $2, $3, $4::jsonb, TRUE, NOW(), NOW())
ON CONFLICT (org_id, role_name) DO NOTHING
RETURNING `+roleColumns,
			id, orgID, roleName, string(permsJSON))

		scanned, err := scanRole(row)
		if err != nil {
			if errors.Is(err, ErrNotFound) {
				// ON CONFLICT DO NOTHING ate the insert.
				return ErrAlreadyExists
			}
			return err
		}
		role = scanned
		return nil
	})
	if err != nil {
		return nil, err
	}
	return role, nil
}

// Update replaces a role's permissions. The default roles' is_custom flag
// stays false but their permissions can still be edited (that's what the
// migration's seed comment implies — admins should be able to expand or
// restrict the built-in roles).
func (r *Repository) Update(ctx context.Context, params UpdateParams) (*Role, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("rbac repository not configured")
	}
	if err := validateCapabilities(params.Permissions); err != nil {
		return nil, err
	}

	permsJSON, err := json.Marshal(params.Permissions)
	if err != nil {
		return nil, fmt.Errorf("encode permissions: %w", err)
	}

	var role *Role
	err = r.db.WithOrgScope(ctx, params.OrgID, func(tx pgx.Tx) error {
		row := tx.QueryRow(ctx, `
UPDATE org_role_mappings
SET permissions = $3::jsonb, updated_at = NOW()
WHERE org_id = $1 AND role_name = $2
RETURNING `+roleColumns,
			params.OrgID, params.RoleName, string(permsJSON))

		scanned, err := scanRole(row)
		if err != nil {
			return err
		}
		role = scanned
		return nil
	})
	if err != nil {
		return nil, err
	}
	return role, nil
}

// Delete removes a custom role. Returns ErrCannotDelete for non-custom
// (seeded) roles — these are part of the contract.
func (r *Repository) Delete(ctx context.Context, orgID, roleName string) error {
	if r == nil || r.db == nil {
		return fmt.Errorf("rbac repository not configured")
	}
	return r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		tag, err := tx.Exec(ctx,
			`DELETE FROM org_role_mappings WHERE org_id = $1 AND role_name = $2 AND is_custom = TRUE`,
			orgID, roleName)
		if err != nil {
			return fmt.Errorf("delete role: %w", err)
		}
		if tag.RowsAffected() == 0 {
			// Either the role doesn't exist OR it's a default role. Look it
			// up inside the same org-scoped transaction (no second
			// WithOrgScope round trip) to tell the two cases apart.
			row := tx.QueryRow(ctx, `
SELECT `+roleColumns+`
FROM org_role_mappings
WHERE org_id = $1 AND role_name = $2`, orgID, roleName)
			if _, err := scanRole(row); err != nil {
				if errors.Is(err, ErrNotFound) {
					return ErrNotFound
				}
				return err
			}
			return ErrCannotDelete
		}
		return nil
	})
}

// AssignMemberRole updates organization_members.role for a single member.
// Returns the post-update assignment shape. Validates the role exists
// before applying — silent assignment to a missing role would create a
// dangling reference. Both checks run inside one org-scoped transaction so
// the existence check and the update are atomic and RLS-enforced together.
func (r *Repository) AssignMemberRole(ctx context.Context, orgID, userID, roleName string) (*MemberRoleAssignment, error) {
	if r == nil || r.db == nil {
		return nil, fmt.Errorf("rbac repository not configured")
	}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		// Validate the role exists for this org.
		row := tx.QueryRow(ctx, `
SELECT `+roleColumns+`
FROM org_role_mappings
WHERE org_id = $1 AND role_name = $2`, orgID, roleName)
		if _, err := scanRole(row); err != nil {
			if errors.Is(err, ErrNotFound) {
				return fmt.Errorf("role %q not defined for org %q: %w", roleName, orgID, ErrNotFound)
			}
			return err
		}

		tag, err := tx.Exec(ctx,
			`UPDATE organization_members
			 SET role = $3, joined_at = COALESCE(joined_at, NOW())
			 WHERE org_id = $1 AND user_id = $2`,
			orgID, userID, roleName)
		if err != nil {
			return fmt.Errorf("assign member role: %w", err)
		}
		if tag.RowsAffected() == 0 {
			return fmt.Errorf("member %q not in org %q: %w", userID, orgID, ErrNotFound)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}

	return &MemberRoleAssignment{
		OrgID:  orgID,
		UserID: userID,
		Role:   roleName,
	}, nil
}

// validateCapabilities rejects unknown capability strings. Empty array is
// allowed (a role with no permissions is a placeholder).
func validateCapabilities(perms []string) error {
	if len(perms) == 0 {
		return nil
	}
	valid := CatalogKeys()
	for _, p := range perms {
		p = strings.TrimSpace(p)
		if p == "" {
			continue
		}
		if _, ok := valid[p]; !ok {
			return fmt.Errorf("%w: %q", ErrInvalidCapability, p)
		}
	}
	return nil
}

func randomSuffix(n int) string {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		return "x"
	}
	return hex.EncodeToString(b)
}

type rowScanner interface {
	Scan(dest ...any) error
}

func scanRole(row rowScanner) (*Role, error) {
	var r Role
	var permsBytes []byte
	if err := row.Scan(&r.ID, &r.OrgID, &r.RoleName, &permsBytes, &r.IsCustom, &r.CreatedAt, &r.UpdatedAt); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("scan role: %w", err)
	}
	if len(permsBytes) > 0 {
		if err := json.Unmarshal(permsBytes, &r.Permissions); err != nil {
			return nil, fmt.Errorf("decode permissions: %w", err)
		}
	} else {
		r.Permissions = []string{}
	}
	return &r, nil
}
